// Copyright 2022 The Blaze Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use blaze_jni_bridge::*;
use datafusion::prelude::SessionContext;
use jni::objects::{JObject, JThrowable};
use once_cell::sync::OnceCell;
use std::any::Any;
use std::error::Error;
use std::fmt::Debug;
use std::panic::AssertUnwindSafe;

mod exec;
mod metrics;
mod rt;

#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

static SESSION: OnceCell<SessionContext> = OnceCell::new();

fn handle_unwinded(err: Box<dyn Any + Send>) {
    // default handling:
    //  * caused by Interrupted/TaskKilled: do nothing but just print a message.
    //  * other reasons: wrap it into a RuntimeException and throw.
    //  * if another error happens during handling, kill the whole JVM instance.
    let recover = || {
        if !is_task_running() {
            // only handle running task
            return Ok(());
        }
        let panic_message = panic_message::panic_message(&err);

        // throw jvm runtime exception
        if jni_exception_check!()? {
            let throwable = jni_exception_occurred!()?;
            jni_exception_clear!()?;
            throw_runtime_exception(panic_message, throwable.as_obj())?;
        } else {
            throw_runtime_exception(panic_message, JObject::null())?;
        };
        Ok(())
    };
    recover().unwrap_or_else(|err: Box<dyn Error>| {
        jni_fatal_error!(format!(
            "Error recovering from panic, cannot resume: {:?}",
            err
        ));
    });
}

fn handle_unwinded_scope<T: Default, E: Debug>(scope: impl FnOnce() -> Result<T, E>) -> T {
    match std::panic::catch_unwind(AssertUnwindSafe(|| scope().unwrap())) {
        Ok(v) => v,
        Err(err) => {
            handle_unwinded(err);
            T::default()
        }
    }
}

fn throw_runtime_exception(msg: &str, cause: JObject) -> datafusion::error::Result<()> {
    let msg = jni_new_string!(msg)?;
    let e = jni_new_object!(JavaRuntimeException(msg.as_obj(), cause))?;

    if let Err(err) = jni_throw!(JThrowable::from(e.as_obj())) {
        jni_fatal_error!(format!(
            "Error throwing RuntimeException, cannot result: {:?}",
            err
        ));
    }
    Ok(())
}
