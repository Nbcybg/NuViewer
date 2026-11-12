// 3D model viewer: one directional "headlight" with a floor of ambient light, two-sided.
// A part of a model can be coloured by a texture and/or per-vertex colours.
cbuffer Constants : register(b0)
{
    row_major float4x4 mvp;    // model -> clip
    row_major float4x4 vrot;   // model -> view rotation (for normals)
    float4 light;              // direction towards the light, view space
    float4 color;              // surface colour (sRGB numbers, like the CPU preview)
    float4 flags;              // x: sample the texture, y: multiply by the vertex colour, z: texture alpha cuts holes
};

Texture2D tex : register(t0);          // R8G8B8A8_UNORM holding sRGB numbers (no gamma conversion on sampling)
SamplerState samp : register(s0);

struct VSIn  { float3 p : POSITION; float3 n : NORMAL; float2 uv : TEXCOORD0; float4 c : COLOR0; };
struct VSOut { float4 pos : SV_POSITION; float3 nv : TEXCOORD0; float2 uv : TEXCOORD1; float4 c : COLOR0; };

VSOut vs(VSIn i)
{
    VSOut o;
    o.pos = mul(float4(i.p, 1.0), mvp);
    o.nv  = mul(float4(i.n, 0.0), vrot).xyz;
    o.uv  = i.uv;
    o.c   = i.c;
    return o;
}

float4 ps(VSOut i) : SV_Target
{
    float3 base = color.rgb;
    float alpha = 1.0;
    if (flags.y > 0.5) { base *= i.c.rgb; alpha *= i.c.a; }
    if (flags.x > 0.5)
    {
        float4 t = tex.Sample(samp, i.uv);
        base *= t.rgb;
        alpha *= t.a;
    }
    if (flags.z > 0.5) clip(alpha - 0.5);
    float3 n = normalize(i.nv);
    float s = 0.24 + 0.76 * abs(dot(n, normalize(light.xyz)));
    // The render target is an sRGB view: write linear values. The CPU preview shades in sRGB space, so match it.
    return float4(pow(saturate(base * s), 2.2), 1.0);
}
