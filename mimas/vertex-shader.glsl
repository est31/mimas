#version 140
in vec2 tex_pos;
in uint tex_ind;
in vec3 position;
in vec3 normal;

out float vlamb;
out vec2 vtex_pos;
flat out uint vtex_ind;
out vec4 vposition;

uniform mat4 pmatrix;
uniform mat4 vmatrix;

const vec3 dir_light_a = normalize(vec3(0.0, -1.0, -1.0));
const vec3 dir_light_b = normalize(vec3(0.0, 1.0, 1.0));

void main() {
	vposition = vmatrix * vec4(position, 1.0);
	gl_Position = pmatrix * vposition;
	vec3 nnormal = normalize(normal);

	// Lambertian shading
	vlamb = max(dot(dir_light_a, nnormal), 0.2) +
		max(dot(dir_light_b, nnormal), 0.2);
	vtex_pos = tex_pos;
	vtex_ind = tex_ind;
}
