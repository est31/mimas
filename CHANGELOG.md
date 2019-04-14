# Changelog

## Release 0.2 - April 14, 2019

Minimum required Rust version: 1.35.0

* Map saving and loading
* Configurability via a `settings.toml` file
* QUIC based network protocol (using the quinn crate)
* Authentication via password
* Rudimentary player avatars
* Mapgen changes:
  - Large macro caves
  - Desert biome with cactuses
  - Iron ore
  - Made ore clusters not 100% filled with the ore

## Release 0.1 - January 8, 2019

Minimum required Rust version: 1.33.0

Initial (internal) release. Featuring:

* Glium based renderer
* Ability to walk around, jump, collide with scene, ...
* Ability to dig/place multiple types of blocks
* Multiplayer support
* Chat support
* Map generation of a landscape with:
  - trees
  - water
  - caves and coal
