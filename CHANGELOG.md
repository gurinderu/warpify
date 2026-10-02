# Changelog

## [0.3.0](https://github.com/gurinderu/warpify/compare/v0.2.0...v0.3.0) (2026-10-02)


### ⚠ BREAKING CHANGES

* `warpify attach` and its background helper are removed.

### Features

* the plugin binds a newly connected client itself ([#11](https://github.com/gurinderu/warpify/issues/11)) ([ead30ee](https://github.com/gurinderu/warpify/commit/ead30ee2408654f87164cde731c9753e973e2b9a))

## [0.2.0](https://github.com/gurinderu/warpify/compare/v0.1.0...v0.2.0) (2026-10-01)


### ⚠ BREAKING CHANGES

* release assets and the installed plugin file are now `warpify-zellij.wasm` (+ `.sha256`) instead of `warpify.wasm`.

### Features

* name the zellij plugin artifact warpify-zellij.wasm ([#7](https://github.com/gurinderu/warpify/issues/7)) ([b46f460](https://github.com/gurinderu/warpify/commit/b46f46054a2f82e089a395496cbeef5bddb6e5d6))
* nix packages and a home-manager module for warpify ([#9](https://github.com/gurinderu/warpify/issues/9)) ([e7f20c2](https://github.com/gurinderu/warpify/commit/e7f20c26bb4c6dd43179d4ce17863fdd2d516a90))

## 0.1.0 (2026-10-01)


### Features

* bind and attach ([#2](https://github.com/gurinderu/warpify/issues/2)) ([a29c5a1](https://github.com/gurinderu/warpify/commit/a29c5a1bdceca79783bf7ca4dc55f277aecdff15))
* install/uninstall zellij integration and release workflow ([#3](https://github.com/gurinderu/warpify/issues/3)) ([fcf9fac](https://github.com/gurinderu/warpify/commit/fcf9fac40c4c38e71563cd9c8487a9ae0fee62c0))
* plugin and CLI skeleton with state and watch ([#1](https://github.com/gurinderu/warpify/issues/1)) ([9242204](https://github.com/gurinderu/warpify/commit/924220470f8ff26ab59380e1dc05fe2bdb9ff74a))
