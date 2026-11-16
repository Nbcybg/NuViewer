// 360-degree viewer: every pixel of the window looks along a ray; the ray picks a spot in an equirectangular picture.
// Shares the vertex shader (and the constant buffer layout) of quad.hlsl.
cbuffer C : register(b0) {
    float4 rect;
    float4 p;    // x: yaw, y: pitch (radians), z: tan(half horizontal field of view), w: mip level
    float4 tint;
    float4 q;    // x: window aspect (width / height)
    float4 rot;
};
struct VO { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
Texture2D t : register(t0);
SamplerState s : register(s0);

float4 ps(VO i) : SV_Target {
    float2 n = float2(i.uv.x * 2.0 - 1.0, 1.0 - i.uv.y * 2.0);
    float3 d = normalize(float3(n.x * p.z, n.y * p.z / q.x, 1.0));
    float cp = cos(p.y), sp = sin(p.y), cy = cos(p.x), sy = sin(p.x);
    d = float3(d.x, d.y * cp + d.z * sp, -d.y * sp + d.z * cp);   // look up / down
    d = float3(d.x * cy + d.z * sy, d.y, -d.x * sy + d.z * cy);   // turn left / right
    float u = atan2(d.x, d.z) * 0.15915494 + 0.5;
    float v = 0.5 - asin(clamp(d.y, -1.0, 1.0)) * 0.31830989;
    // The mip is chosen from the field of view (not from derivatives: they jump where atan2 wraps and draw a seam).
    return t.SampleLevel(s, float2(u, v), p.w) * tint;
}
