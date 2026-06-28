# Use a native desktop shell with isolated web graphics

The finished application must not be a React website filling a Tauri WebView. V1 will migrate workflow navigation, forms, lists, settings, file operations, review state, and commands into a Rust-native Slint desktop shell.

Web rendering remains permitted only for capabilities whose required platform is web-native: the Gaode JavaScript 3D map and the existing Three.js voxel preview. Each runs as an isolated, purpose-specific graphics surface behind a typed Rust message boundary. Neither surface owns application navigation, persistence, business rules, credentials, or export logic.

Migration is incremental. The existing Tauri/React implementation remains a reference and temporary compatibility shell until the native workflow reaches feature parity. New domain and generation logic belongs in Rust crates or UI-independent contracts, not new React-only state.
