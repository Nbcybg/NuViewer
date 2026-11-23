// Spine skeletons drawn on the GPU: textured triangles with a light colour and (optionally) a dark colour per vertex.
// Mode 3 draws a flat premultiplied colour (the outline of a highlighted layer).
// The pages are uploaded premultiplied; the pixel shader gives a premultiplied colour, the blend states do the four Spine blend modes.
cbuffer C : register(b0) {
    float4 vp; // x, y: size of the target in pixels
};
struct VI { float2 pos : POSITION; float2 uv : TEXCOORD0; float4 light : COLOR0; float4 dark : COLOR1; };
struct VO { float4 pos : SV_Position; float2 uv : TEXCOORD0; float4 light : COLOR0; float4 dark : COLOR1; };

VO vs(VI i) {
    VO o;
    o.pos = float4(i.pos.x / vp.x * 2.0 - 1.0, 1.0 - i.pos.y / vp.y * 2.0, 0.0, 1.0);
    o.uv = i.uv;
    o.light = i.light;
    o.dark = i.dark;
    return o;
}

Texture2D tex : register(t0);
SamplerState smp : register(s0);

float4 ps(VO i) : SV_Target {
    float4 T = tex.Sample(smp, i.uv); // premultiplied
    float3 c = i.light.rgb;
    float ca = i.light.a;
    int mode = (int)(i.dark.w + 0.5); // 0: one colour, 1: two colours on a premultiplied page, 2: two colours on a straight page
    float4 s;
    s.a = T.a * ca;
    if (mode == 3) {
        // a flat colour (outlines): no texture at all
        s = float4(c * ca, ca);
    } else if (mode == 0) {
        s.rgb = T.rgb * c * ca;
    } else if (mode == 1) {
        s.rgb = (T.rgb * c + (T.a - T.rgb) * i.dark.rgb) * ca;
    } else {
        float3 t = T.rgb / max(T.a, 0.0001);
        s.rgb = ((1.0 - t) * i.dark.rgb + t * c) * s.a;
    }
    return s;
}
