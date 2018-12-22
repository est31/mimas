#version 140
in vec4 vcolor;
in vec4 vposition;

out vec4 fcolor;

const vec4 fog = vec4(0.5, 0.5, 0.5, 1.0);

void main() {
	float fog_factor = clamp((length(vposition) - 60.0) / 40.0, 0.0, 1.0);
	fcolor = mix(vcolor, fog, fog_factor);
}
