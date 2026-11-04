struct Vertex { float4 position : SV_POSITION; };
Vertex vs_main(uint id : SV_VertexID) {
    Vertex v;
    float2 uv = float2((id << 1) & 2, id & 2);
    v.position = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return v;
}
Texture2D desktop_image : register(t0);
Texture2D pointer_image : register(t1);
cbuffer Params : register(b0) {
    int width; int height; int rotation; int pointer_kind;
    int pointer_x; int pointer_y; int pointer_width; int pointer_height;
};
float4 ps_main(Vertex v) : SV_TARGET {
    int2 p = int2(v.position.xy);
    int2 source = p;
    // DXGI fornece a superfície sem rotação; a saída segue a orientação do monitor.
    if (rotation == 2) source = int2(p.y, width - 1 - p.x);
    if (rotation == 3) source = int2(width - 1 - p.x, height - 1 - p.y);
    if (rotation == 4) source = int2(height - 1 - p.y, p.x);
    float3 color = desktop_image.Load(int3(source, 0)).rgb;
    int2 pointer_p = p - int2(pointer_x, pointer_y);
    if (pointer_kind != 0 && all(pointer_p >= 0) && pointer_p.x < pointer_width && pointer_p.y < pointer_height) {
        float4 cursor = pointer_image.Load(int3(pointer_p, 0));
        uint3 background = uint3(round(saturate(color) * 255));
        uint3 foreground = uint3(round(cursor.rgb * 255));
        if (pointer_kind == 1) {
            // Monocromático: primeiro AND, depois XOR (inclui inversão do fundo).
            uint mask = foreground.r;
            uint invert = foreground.g;
            color = float3((background & mask) ^ invert) / 255.0;
        } else if (pointer_kind == 4) {
            // Masked color: alpha 0 substitui; alpha 255 faz XOR.
            color = cursor.a > 0.5 ? float3(background ^ foreground) / 255.0 : cursor.rgb;
        } else {
            // Cursor color: mistura alpha, como no exemplo oficial DXGI.
            color = cursor.rgb * cursor.a + color * (1 - cursor.a);
        }
    }
    return float4(color, 1);
}
