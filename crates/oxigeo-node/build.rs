//! Build script for `oxigeo-node`: runs the `napi-build` setup step so the
//! generated N-API bindings link correctly against the Node.js runtime.

extern crate napi_build;

fn main() {
    napi_build::setup();
}
