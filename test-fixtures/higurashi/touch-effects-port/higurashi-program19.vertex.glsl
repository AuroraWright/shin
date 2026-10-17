#version 440
#extension GL_ARB_explicit_attrib_location : require
#extension GL_ARB_explicit_uniform_location : require
#extension GL_ARB_shading_language_420pack : require

layout(binding=0) uniform vcb0 {
	vec4 hlslcc_mtx4x4proj[4];
};
layout(location = 0) in  vec4 in_POSITION0;
out vec2 vs_TEXCOORD0;
vec4 u_xlat0;
void main()
{
    u_xlat0 = in_POSITION0.yyyy * hlslcc_mtx4x4proj[1];
    u_xlat0 = hlslcc_mtx4x4proj[0] * in_POSITION0.xxxx + u_xlat0;
    gl_Position = u_xlat0 + hlslcc_mtx4x4proj[3];
    vs_TEXCOORD0.xy = in_POSITION0.zw;
    return;
}
