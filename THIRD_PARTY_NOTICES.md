# Third-party notices

Campus Reconstruction Tool V1 uses the following major open-source components:

- Slint, used under the Slint Royalty-free License for the native application UI. https://slint.dev
- Arnis 2.9.0, Apache-2.0, copyright Louis Erbkamm. The adapted source and upstream record are under `native/crates/arnis-core/`.
- winit, wry, WebView2 bindings, pixels/wgpu, serde, Tokio, reqwest, fastnbt, and flate2 under their respective repository licenses.

The high-de map window loads the Gaode Web JS API at runtime and requires credentials supplied by the user. Credentials are stored in Windows Credential Manager.
