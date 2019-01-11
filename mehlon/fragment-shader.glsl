#version 140
in vec4 vcolor;
in vec4 vposition;

out vec4 fcolor;

uniform vec2 fog_near_far;

const vec4 fog = vec4(0.5, 0.5, 0.5, 1.0);

void main() {
	float fog_factor = clamp((length(vposition) - fog_near_far.y) / fog_near_far.x, 0.0, 1.0);
	fcolor = mix(vcolor, fog, fog_factor);
}
