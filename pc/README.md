# Segestria — PC (Tauri)

Frontend de escritorio del launcher. Es una capa fina: la ventana y la UI
(`src/`, HTML/CSS/JS vanilla) más los comandos de Tauri (`src-tauri/src/lib.rs`)
que exponen la lógica real, que vive en [`../core`](../core).

## Requisitos

- [Rust](https://rustup.rs/) (toolchain estable)
- [Node.js](https://nodejs.org/) 18+

## Correrlo en desarrollo

```sh
npm install
npm run tauri dev
```

La primera compilación tarda unos minutos (compila todo el workspace de Rust,
`core` incluido). Las siguientes son incrementales.

## Build de release

```sh
npm run tauri build
```

Genera el instalador (NSIS en Windows) en `target/release/bundle` — en la raíz
del workspace, no dentro de `pc/`, porque todo el workspace de Rust comparte un
solo `target/`.

## Recomendado para el IDE

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
