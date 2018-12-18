#version 140
in vec3 position;
in vec4 color;

out vec4 vcolor;

uniform mat4 pmatrix;
uniform mat4 vmatrix;
void main() {
	vcolor = color;
	gl_Position = pmatrix * vmatrix * vec4(position, 1.0);
}
