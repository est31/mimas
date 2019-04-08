## Mehlon

Mehlon is a WIP voxel engine and game, inspired a little by Minecraft and a lot by Minetest.

For usage instructions, please view [USAGE.md](USAGE.md).

### Origin of the name

For the name, "meh" was too short and too much taken so I appended "lon" and
ended up getting a transliteration of the hindi `महलों` which seems to be the plural form of `महल` "mahal", meaning palace. So I guess I'll name this thing after the hindi word for palaces.

### Supporting work

In order to be able to build mehlon, three crates have been published:

* [glium-glyph](https://github.com/est31/glium-glyph), so that mehlon can render text
* [serde-big-array](https://github.com/est31/serde-big-array), to work around the current 32 elements restriction of Rust
* [rcgen](https://github.com/est31/rcgen/), to create certificates for quinc network communication

### Compiling / Running

In order to compile and run mehlon, you'll need the [Rust](https://github.com/rust-lang/rust) compiler.

Either, you can obtain it from your distro, or you can use [rustup](https://rustup.rs/).

#### Compiling with stable Rust

Released versions of Mehlon will be able to compile on stable Rust.
Here, the currently latest stable version of Rust will be targeted.

The git version of Mehlon however uses one feature from Rust nightly: [cargo profile dependencies](https://github.com/rust-lang/rust/issues/48683).
This feature is used to enable a better edit-compile-run cycle,
as the git version is targeted at development.
Thus, on git, per default, the nightly channel of Rust is required.

However, if you want to compile mehlon on stable Rust,
you can easily edit `Cargo.toml` to compile it yourself.

### License

Licensed under the MPL 2.0. For details, see the [LICENSE](LICENSE) file.
