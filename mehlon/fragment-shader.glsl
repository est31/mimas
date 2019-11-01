#version 140
in float vlamb;
in vec2 vtex_pos;
flat in uint vtex_ind;
in vec4 vposition;

out vec4 fcolor;

uniform sampler2DArray texture_arr;
uniform vec2 fog_near_far;

const vec4 fog = vec4(0.5, 0.5, 0.5, 1.0);

void main() {
	vec4 tcolor = texture(texture_arr, vec3(vtex_pos, vtex_ind));
	if (tcolor.a < 0.5) {
		discard;
	}

	vec4 color_lamb = vlamb * tcolor;
	color_lamb.a = tcolor.a;
	float fog_factor = clamp((length(vposition) - fog_near_far.y) / fog_near_far.x, 0.0, 1.0);
	fcolor = mix(color_lamb, fog, fog_factor);
}
