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
