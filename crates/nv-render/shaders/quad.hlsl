cbuffer C : register(b0) {
    float4 rect; // left, top, right, bottom in NDC
    float4 p;    // x: texture width, y: texture height, z: mip level, w: 1 = nearest, 0 = bicubic
    float4 tint; // multiplied into the result (linear RGBA); white = unchanged
    float4 q;    // x: EXIF orientation 1..8 (0 or 1 = upright)
};
struct VO { float4 pos : SV_Position; float2 uv : TEXCOORD0; };

VO vs(uint id : SV_VertexID) {
    float2 uv = float2(id & 1, id >> 1);
    VO o;
    o.pos = float4(lerp(rect.x, rect.z, uv.x), lerp(rect.y, rect.w, uv.y), 0, 1);
    o.uv = uv;
    return o;
}

Texture2D t : register(t0);

float4 ld(int2 c, int2 sz, int lod) {
    return t.Load(int3(clamp(c, int2(0, 0), sz - 1), lod));
}

// The CPU picks the mip level so the remaining scale is in [0.5, 1) when shrinking; here we resample that
// mip with Catmull-Rom (sharp, little ringing). Texture is an sRGB view, so filtering happens in linear light.
float4 ps(VO i) : SV_Target {
    int lod = (int)p.z;
    int2 sz = max(int2(p.xy) >> lod, int2(1, 1));
    // Orientation: map the displayed quad's uv back to the stored (unrotated) pixels.
    float2 uv = i.uv;
    int o = (int)q.x;
    if (o == 2) uv = float2(1 - uv.x, uv.y);
    else if (o == 3) uv = float2(1 - uv.x, 1 - uv.y);
    else if (o == 4) uv = float2(uv.x, 1 - uv.y);
    else if (o == 5) uv = float2(uv.y, uv.x);
    else if (o == 6) uv = float2(uv.y, 1 - uv.x);
    else if (o == 7) uv = float2(1 - uv.y, 1 - uv.x);
    else if (o == 8) uv = float2(1 - uv.y, uv.x);
    float2 pos = uv * float2(sz);
    if (p.w > 0.5) {
        return ld(int2(floor(pos)), sz, lod) * tint;
    }
    float2 c = pos - 0.5;
    float2 f = frac(c);
    int2 b = int2(floor(c));
    float2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    float2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    float2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    float2 w3 = f * f * (-0.5 + 0.5 * f);
    float wx[4] = { w0.x, w1.x, w2.x, w3.x };
    float wy[4] = { w0.y, w1.y, w2.y, w3.y };
    float4 acc = 0;
    [unroll] for (int y = 0; y < 4; y++) {
        float4 row = 0;
        [unroll] for (int x = 0; x < 4; x++) {
            row += wx[x] * ld(b + int2(x - 1, y - 1), sz, lod);
        }
        acc += wy[y] * row;
    }
    return saturate(acc) * tint;
}
