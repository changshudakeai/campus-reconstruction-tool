# Use a native desktop shell with isolated web graphics

The finished application must not be a React website filling a Tauri WebView. V1 will migrate workflow navigation, forms, lists, settings, file operations, review state, and commands into a Rust-native Slint desktop shell.

Web rendering remains permitted on desktop only for the Gaode JavaScript 3D map, whose official 3D capability requires a browser environment. It opens as an independent tool window rather than an embedded region of the Slint layout and communicates through a typed Rust message boundary. It does not own application navigation, persistence, business rules, credentials, or export logic.

The independent map window is one modal product task, not a detached developer tool. It owns all immediate interaction feedback for the task it displays, including validation, loading, retry, cancel, and successful completion. The main window cannot be the sole destination for an error while the map window covers it. Completion or cancellation returns focus to the originating workspace and advances or preserves that task explicitly.

The Minecraft schematic preview migrates from Three.js to a Rust-native renderer and therefore does not retain a desktop WebView. A separately hosted Cloud Web Companion may reuse web presentation code, but it is never embedded into the desktop application or used as its fallback.

The Cloud Web Companion is delivered only after native desktop migration and parity verification. It may reuse web domain adapters and presentation assets, but not the legacy button placement or long-page information architecture. Its later redesign follows the same action hierarchy as the native edition without blocking the desktop cutover.

Migration is incremental. The existing Tauri/React implementation remains a development-only reference until the native workflow reaches feature parity. It is removed from release builds after migration verification. Embedding a Wry child WebView inside the Slint window may be investigated after V1, but it is not allowed to block or reshape the native migration. New domain and generation logic belongs in Rust crates or UI-independent contracts, not new React-only state.

The migration uses an atomic product cutover. Implementation may proceed through ordered internal stages, but no partially native desktop edition is released. The native edition replaces the compatibility implementation only after current desktop capabilities have verified functional parity, including both reconstruction modes, persistence, map review, generation, preview, editing, and export.

Functional parity covers every currently user-visible and operable desktop capability, its persisted project data, and its export result. Unreachable branches, permanently hidden controls, obsolete debug panels, and duplicate legacy entry points are not parity requirements and are removed rather than migrated. No visible advanced capability may be dropped merely because it is infrequently used.

Visual parity is not required. The native edition preserves the Foundation/Detailed domain split, accepted workflow order, terminology, data meaning, and advanced-versus-primary hierarchy while reorganising the interaction around native desktop conventions. It should reduce long scrolling, nested disclosures, excessive buttons, and actions scattered far from the object they affect.

Each workflow surface has at most one high-emphasis primary action, placed in a stable footer. Object-scoped actions remain beside the affected candidate or feature. Undo, redo, and save belong to the top toolbar; import, export, and settings belong to application menus; infrequent actions belong to a More menu. Destructive actions are visually and spatially separated from the primary action.

The first native release is supported and acceptance-tested on Windows 10/11 x64. Architecture and domain crates should avoid unnecessary platform coupling, but macOS and Linux packaging, WebView behavior, graphics verification, and support are outside the migration acceptance gate.

The desktop application uses Slint under the Royalty-Free Desktop, Mobile, and Web Applications License rather than imposing GPLv3 on the project. The native About surface includes Slint's `AboutSlint` attribution widget as required by that licence. The application's own source and distribution licence remains a separate project decision.

Rust owns one Desktop Application State and the versioned project persistence boundary. Slint renders projections of that state and emits user intentions. The Gaode tool window returns typed map results; the native schematic renderer receives immutable render snapshots and returns inspection events. No presentation surface keeps a competing authoritative project, review, candidate, or generation state.

The Gaode map and native schematic preview run as separately packaged Desktop Tool Processes supervised by the Slint application. This isolates the Slint, WebView2, and GPU event loops and prevents a map or renderer crash from losing project editing state. Tool processes are implementation components of one installed application and are never started or managed manually by the user.

On Windows, the main application communicates with tool processes through per-session named pipes using a length-prefixed, versioned JSON protocol and a random session token. Tool processes receive only the immutable snapshot needed for their task and cannot read project files or credentials directly. The main application supervises their lifetime and terminates them on exit.

Cutover requires all items in the maintained [V1 functional parity matrix](../v1-functional-parity.md) to pass; successful migration of existing projects; complete real-campus runs through Foundation and Detailed modes; verified map, native preview, editing, and export; installation, uninstallation, recovery, and offline checks; an installer below the 50 MB budget; and no blocking crash during an extended workflow. The current desktop entry point changes only after automated evidence and final user experience acceptance.
