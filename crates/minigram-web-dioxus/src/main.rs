#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!(
        "minigram-web-dioxus is a browser client. Build for wasm32-unknown-unknown and serve with `dx serve`."
    );
}

#[cfg(target_arch = "wasm32")]
mod api;
#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod components;
#[cfg(target_arch = "wasm32")]
mod models;

#[cfg(target_arch = "wasm32")]
fn main() {
    dioxus_web::launch(app::App);
}
