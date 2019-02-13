# Changelog

## Release (Upcoming)

* Map saving and loading
* Added configurability via a `settings.toml` file
* QUIC based network protocol (using the quinn crate)
* Mapgen changes:
  - Macro caves
  - Desert biome with cactuses

## Release 0.1 - January 8, 2019

Required Rust version: 1.33.0

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
