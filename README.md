## Mimas

Mimas is a WIP voxel engine and game, inspired a little by Minecraft and a lot by Minetest.

For usage instructions, please view [USAGE.md](USAGE.md).

### Origin of the name

The game is named after the Saturn moon [Mimas](https://en.wikipedia.org/wiki/Mimas_(moon)).

### Supporting work

In order to be able to build mimas, three crates have been published:

* [glium-glyph](https://github.com/est31/glium-glyph), so that mimas can render text
* [serde-big-array](https://github.com/est31/serde-big-array), to work around the current 32 elements restriction of Rust
* [rcgen](https://github.com/est31/rcgen/), to create certificates for quic network communication

### Compiling / Running

In order to compile and run mimas, you'll need the [Rust](https://github.com/rust-lang/rust) compiler.

Either, you can obtain it from your distro, or you can use [rustup](https://rustup.rs/).

### License

Licensed under the MPL 2.0. For details, see the [LICENSE](LICENSE) file.
