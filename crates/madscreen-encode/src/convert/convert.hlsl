struct Vertex { float4 position : SV_POSITION; float2 uv : TEXCOORD0; };
Vertex vs_main(uint id : SV_VertexID) {
    Vertex o;
    o.uv = float2((id << 1) & 2, id & 2);
    o.position = float4(o.uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}
Texture2D image : register(t0);
SamplerState linear_sampler : register(s0);
cbuffer Params : register(b0) { uint frame_index; uint width; uint height; uint unused; };
float3 rgb(float2 uv) {
    [branch] if ((unused & 1u) == 0u) return image.Sample(linear_sampler, uv).rgb;
    uint source_width, source_height;
    image.GetDimensions(source_width, source_height);
    float2 output_size = float2(width, height) * (((unused & 2u) != 0u) ? 2.0 : 1.0);
    [branch] if (source_width <= output_size.x && source_height <= output_size.y)
        return image.Sample(linear_sampler, uv).rgb;
    float3 sum = 0;
    [unroll] for (uint y = 0; y < 4; y++) {
        [unroll] for (uint x = 0; x < 4; x++) {
            float2 offset = (float2(x, y) / 4.0 - 0.375) / output_size;
            sum += image.Sample(linear_sampler, uv + offset).rgb;
        }
    }
    return sum / 16.0;
}
float4 ps_bgra(Vertex v) : SV_TARGET { return float4(rgb(v.uv), 1); }
// BT.709 SDR RGB full -> Y'CbCr limited (16..235, 16..240), arredondamento por UNORM.
float ps_y(Vertex v) : SV_TARGET {
    return (16.0 + 219.0 * dot(rgb(v.uv), float3(0.2126, 0.7152, 0.0722))) / 255.0;
}
float2 ps_uv(Vertex v) : SV_TARGET {
    // Média 2x2 no espaço de saída, amostragem bilinear na entrada.
    float2 d = 0.25 / float2(width, height);
    float3 c = (rgb(v.uv + float2(-d.x,-d.y)) + rgb(v.uv + float2(d.x,-d.y))
              + rgb(v.uv + float2(-d.x,d.y)) + rgb(v.uv + float2(d.x,d.y))) * 0.25;
    return (128.0 + 224.0 * float2(dot(c, float3(-0.114572,-0.385428,0.5)),
                                  dot(c, float3(0.5,-0.454153,-0.045847)))) / 255.0;
}
float4 ps_synthetic(Vertex v) : SV_TARGET {
    uint2 p = uint2(v.position.xy);
    uint n = (p.x * 1664525u + p.y * 1013904223u + frame_index * 747796405u);
    n ^= n >> 16;
    uint bar = ((p.x + frame_index * 8u) * 8u / max(width, 1u)) & 7u;
    float3 c = float3((bar >> 2) & 1u, (bar >> 1) & 1u, bar & 1u);
    c = saturate(c * 0.8 + float(n & 255u) / 255.0 * 0.2);
    // Contador binário visível, baseado no índice do frame.
    if (p.y < 48u && p.x < 32u * 24u) c = ((frame_index >> (p.x / 24u)) & 1u) ? 1.0 : 0.0;
    return float4(c, 1);
}
// Quadro autossuficiente para clientes novos/antigos, sem textura da captura.
float4 ps_pause(Vertex v) : SV_TARGET {
    static const uint glyphs[19] = { 4329631u, 9616943u, 18415150u, 18667121u, 16267326u, 18405233u, 32641183u, 16267326u, 16267326u, 18415150u, 15255086u, 0u, 1097263u, 18415150u, 15255089u, 16267326u, 18415150u, 16303663u, 18415150u };
    uint scale = max(1u, min(width / 120u, height / 20u));
    int2 origin = (int2(width, height) - int2(114u * scale, 5u * scale)) / 2;
    int2 p = int2(v.position.xy) - origin;
    bool ink = false;
    if (p.x >= 0 && p.y >= 0) {
        uint2 cell = uint2(p) / scale;
        uint letter = cell.x / 6u;
        if (letter < 19u && cell.y < 5u && cell.x % 6u < 5u) {
            ink = ((glyphs[letter] >> (cell.y * 5u + cell.x % 6u)) & 1u) != 0u;
        }
    }
    return float4(ink ? float3(0.9, 0.9, 0.9) : float3(0.035, 0.035, 0.035), 1);
}
