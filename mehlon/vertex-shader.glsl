#version 140
in vec3 position;
in vec4 color;
in vec3 normal;

out vec4 vcolor;
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
	float fac = max(dot(dir_light_a, nnormal), 0.2) +
		max(dot(dir_light_b, nnormal), 0.2);
	vcolor = fac * color;
	vcolor.a = color.a;
}
