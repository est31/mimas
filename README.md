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

Mehlon only requires stabilized Rust features, but currently
requires 1.41 which hasn't been released yet. For the time
being, I suggest using nightly, then beta once 1.41 moves there,
and finally stable (on Jan 30, 2020).

If you still want to compile mehlon on stable Rust,
you can easily edit `Cargo.toml` to compile it yourself.

### License

Licensed under the MPL 2.0. For details, see the [LICENSE](LICENSE) file.
