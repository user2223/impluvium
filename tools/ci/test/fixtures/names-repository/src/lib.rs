impl Drop for Guard { fn drop(&mut self) {} }
impl<T> std::ops::Drop for Holder<T> {
Drop::drop(&mut value);
// Drop the cache
let dir = "drop-debug";
let package = "drop-app";
use drop_app_lib::run;
