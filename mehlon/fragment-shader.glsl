#version 140
in float vlamb;
in vec4 vcolor;
in vec4 vposition;

out vec4 fcolor;

uniform vec2 fog_near_far;

const vec4 fog = vec4(0.5, 0.5, 0.5, 1.0);

void main() {
	vec4 color_lamb = vlamb * vcolor;
	color_lamb.a = vcolor.a;

	float fog_factor = clamp((length(vposition) - fog_near_far.y) / fog_near_far.x, 0.0, 1.0);
	fcolor = mix(color_lamb, fog, fog_factor);
}
