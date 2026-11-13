// 3D model viewer. One directional light (a headlight or a fixed one, see the settings), two-sided surfaces.
// A part of a model can be coloured by a texture and/or per-vertex colours, and bumped by a normal map.
// Five looks (style.x): 0 combined, 1 diffuse, 2 cartoon, 3 normals, 4 unlit.
// Passes (style.y): 0 normal, 1 floor shadow (flat translucent black), 2 wireframe lines, 3 dimmed solid under the wireframe.
cbuffer Constants : register(b0)
{
    row_major float4x4 mvp;    // model -> clip (for the shadow pass: the model flattened onto the floor)
    row_major float4x4 vrot;   // model -> view rotation (for normals)
    float4 light;              // direction towards the light, view space
    float4 color;              // surface colour (sRGB numbers, like the CPU preview)
    float4 flags;              // x: sample the texture, y: multiply by the vertex colour, z: texture alpha cuts holes, w: use the normal map
    float4 style;              // x: look, y: pass
};

Texture2D tex : register(t0);          // R8G8B8A8_UNORM holding sRGB numbers (no gamma conversion on sampling)
Texture2D nmap : register(t1);         // tangent-space normal map, same format
SamplerState samp : register(s0);

struct VSIn  { float3 p : POSITION; float3 n : NORMAL; float2 uv : TEXCOORD0; float4 c : COLOR0; };
struct VSOut { float4 pos : SV_POSITION; float3 nv : TEXCOORD0; float2 uv : TEXCOORD1; float4 c : COLOR0; float3 pv : TEXCOORD2; };

VSOut vs(VSIn i)
{
    VSOut o;
    o.pos = mul(float4(i.p, 1.0), mvp);
    o.nv  = mul(float4(i.n, 0.0), vrot).xyz;
    o.pv  = mul(float4(i.p, 1.0), vrot).xyz;   // only differences of this matter (the normal map's tangent frame)
    o.uv  = i.uv;
    o.c   = i.c;
    return o;
}

float4 ps(VSOut i) : SV_Target
{
    int look = (int)style.x;
    int stage = (int)style.y;
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
    if (stage == 1) return float4(0.0, 0.0, 0.0, 0.42);
    if (stage == 2) return float4(pow(float3(0.78, 0.90, 1.0), 2.2), 1.0);

    float3 n = normalize(i.nv);
    if (flags.w > 0.5)
    {
        // Tangent frame from screen-space derivatives (no tangents in the vertex data).
        float3 tn = nmap.Sample(samp, i.uv).xyz * 2.0 - 1.0;
        tn.y = -tn.y;                          // maps are authored y-up; our v runs downwards
        float3 dp1 = ddx(i.pv), dp2 = ddy(i.pv);
        float2 du1 = ddx(i.uv), du2 = ddy(i.uv);
        float3 dp2perp = cross(dp2, n), dp1perp = cross(n, dp1);
        float3 T = dp2perp * du1.x + dp1perp * du2.x;
        float3 B = dp2perp * du1.y + dp1perp * du2.y;
        float invmax = rsqrt(max(max(dot(T, T), dot(B, B)), 1e-20));
        n = normalize(T * invmax * tn.x + B * invmax * tn.y + n * tn.z);
    }
    float3 L = normalize(light.xyz);
    float3 rgb;
    if (look == 1)
    {
        // Plain Lambert, two-sided (STL winding is often inconsistent).
        rgb = base * (0.24 + 0.76 * abs(dot(n, L)));
    }
    else if (look == 3)
    {
        rgb = n * 0.5 + 0.5;
    }
    else if (look == 4)
    {
        rgb = base;
    }
    else
    {
        // Face the viewer, so lighting does not depend on which way a triangle happens to be wound.
        if (n.z < 0.0) n = -n;
        float nl = saturate(dot(n, L));
        if (look == 2)
        {
            float band = nl < 0.12 ? 0.38 : (nl < 0.45 ? 0.62 : (nl < 0.78 ? 0.86 : 1.0));
            float edge = n.z < 0.24 ? 0.18 : 1.0;
            rgb = base * band * edge;
        }
        else
        {
            float3 H = normalize(L + float3(0.0, 0.0, 1.0));
            float spec = pow(saturate(dot(n, H)), 48.0) * 0.30 * (nl > 0.0 ? 1.0 : 0.0);
            float rim = pow(1.0 - saturate(n.z), 3.0) * 0.14;
            rgb = base * (0.20 + 0.80 * nl) + spec + rim;
        }
    }
    if (stage == 3) rgb *= 0.5;
    // The render target is an sRGB view: write linear values. The CPU preview shades in sRGB space, so match it.
    return float4(pow(saturate(rgb), 2.2), 1.0);
}
