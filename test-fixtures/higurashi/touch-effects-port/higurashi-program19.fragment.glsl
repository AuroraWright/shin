#version 440
#extension GL_ARB_explicit_attrib_location : require
#extension GL_ARB_explicit_uniform_location : require
#extension GL_ARB_shading_language_420pack : require

layout(binding=0) uniform fcb0 {
	vec4 params[2];
};
layout(location = 0, binding = 0) uniform  sampler2D texture2d_0;
in  vec2 vs_TEXCOORD0;
layout(location = 0) out vec4 SV_Target0;
vec2 u_xlat0;
vec4 u_xlat1;
vec4 u_xlat10_1;
float u_xlat2;
vec2 u_xlat4;
float u_xlat6;
void main()
{
    u_xlat0.xy = vs_TEXCOORD0.xy + (-params[0].xy);
    u_xlat4.xy = u_xlat0.xy * params[0].zw;
    u_xlat4.x = dot(u_xlat4.xy, u_xlat4.xy);
    u_xlat4.x = sqrt(u_xlat4.x);
    u_xlat6 = u_xlat4.x + (-params[1].x);
    u_xlat4.x = min(u_xlat4.x, 1.0);
    u_xlat6 = u_xlat6 * 6.28318548;
    u_xlat6 = sin(u_xlat6);
    u_xlat0.xy = vec2(u_xlat6) * params[1].zw + u_xlat0.xy;
    u_xlat0.xy = u_xlat0.xy + params[0].xy;
    u_xlat10_1 = texture(texture2d_0, u_xlat0.xy);
    u_xlat1 = vec4(u_xlat6) * vec4(0.200000003, 0.200000003, 0.200000003, 0.200000003) + u_xlat10_1;
    u_xlat0.x = u_xlat4.x * u_xlat4.x;
    u_xlat0.x = (-u_xlat4.x) * u_xlat0.x + 1.0;
    u_xlat2 = (-params[1].x) * params[1].x + 1.0;
    u_xlat0.x = u_xlat2 * u_xlat0.x;
    SV_Target0 = u_xlat0.xxxx * u_xlat1;
    return;
}
