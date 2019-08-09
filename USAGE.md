## Usage instructions

### Starting, running, connecting

You can start the client in singleplayer mode by doing:

```
cargo run --release
```

You can also connect it to an existing server:

```
cargo run --release -- --connect <host>:<port>--nick username --password pw
```

E.g. to connect to localhost, you can do:

```
cargo run --release -- --connect 127.0.0.1:7700 --nick tester --password test
```

A server can be started using:

```
cargo run --release -p mehlon-server
```

Per default it only accepts connections from localhost on port 7700.
You can make it accept on other addresses and ports by e.g. doing:

```
cargo run --release -p mehlon-server --listen 0.0.0.0:7700
```

Help on command line params can be obtained using:

```
cargo run --release -- --help
cargo run --release -p mehlon-server -- --help
```

### Settings files

`mehlon` has the ability to read from settings files.
Currently, it reads the `settings.toml` file from the current working directory.
Descriptions of the available settings are obtainable in [settings.toml.example](settings.toml.example).


### Controls

Controls are very similar to minetest controls.

* `w`/`a`/`s`/`d` → movement
* `e` → fast movement (press it while moving)
* `space` → jump (no fly mode) or ascend (fly mode)
* `left shift` → descend (fly mode)

* `h` → toogle noclip mode
* `j` → toogle fast mode
* `k` → toogle fly mode

* `i` → open inventory menu
* `t` → chat

* `esc` → release mouse cursor

* `left click` → dig/mine something
* `right click` → place something
