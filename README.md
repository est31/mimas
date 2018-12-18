## Mehlon

For the name, "meh" was too short and too much taken so I appended "lon" and
ended up getting a transliteration of the hindi `महलों` which seems to be the plural form of `महल` "mahal", meaning palace. So I guess I'll name this thing after the hindi word for palaces.

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
