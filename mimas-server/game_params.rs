use std::sync::Arc;

use mimas_common::game_params::{NameIdMap, ServerGameParamsHdl, load_params_failible};

pub fn load_server_game_params(nm :NameIdMap) -> ServerGameParamsHdl {
    Arc::new(load_params_failible(nm, DEFAULT_GAME_PARAMS_STR)
        .expect("Couldn't load game params"))
}

static DEFAULT_GAME_PARAMS_STR :&str = include_str!("game-params.toml");

#[cfg(test)]
#[test]
fn default_game_params_parse_test() {
	let nm = NameIdMap::builtin_name_list();
	mimas_common::game_params::default_game_params(nm, DEFAULT_GAME_PARAMS_STR).unwrap();
}
