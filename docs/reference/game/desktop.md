# Desktop app

The macOS desktop app is **Under the Canopy**, built with the existing Tauri shell. It embeds the production web build, game content, models, maps, textures, and UI assets; no local development server is required to play singleplayer.

## Build on macOS

Install the project's npm dependencies, Rust/Cargo, and Xcode Command Line Tools, then run:

```sh
npm run build:desktop
```

This runs the TypeScript/content checks and Vite build, then creates `src-tauri/target/release/bundle/macos/Under the Canopy.app` for the host architecture. The existing bundle identifier is retained so local app storage remains compatible. The bundle uses ad-hoc signing for local use; it is not Developer ID signed or notarized for public distribution.

## App icon

The published icon is `assets/library/asset.interface.app-icon.under-the-canopy/image.png`. Native PNG, ICNS and ICO variants live in `src-tauri/icons/` and are included through `src-tauri/tauri.conf.json`. Regenerate them after changing the master artwork:

```sh
npm run tauri -- icon assets/library/asset.interface.app-icon.under-the-canopy/image.png --output src-tauri/icons
```

Multiplayer still needs the configured match server. Development-only content-saving endpoints are not part of the desktop bundle.
