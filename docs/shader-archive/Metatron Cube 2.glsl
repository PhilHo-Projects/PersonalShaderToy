#define S(v) smoothstep( .002, 0., v )
#define PI 3.14159265359

vec2[13] C; // circles for current layer

float drawCircle(vec2 P, float size) {
    float r = size,
          w = .001, // thickness/2.
          d = length(P);
   return S( abs(length(P)-r+w) - w );
}

#define hs(h,s) 1. + s* clamp( abs(fract(h + vec3(3,2,1)/3.) * 6. - 3.) - 2., -1., 0.)

float drawLine(vec2 p, int i, int j, float thickness) {
    float d, w = thickness; // thickness
    vec2 a = C[i],
         b = C[j]-a;
    p -= a;
    d = length(p - b * clamp(dot(p, b) / dot(b,b), 0., 1.) );
    return S( d - w );
}

// 複数のブレンドモード
vec3 softAdditiveBlend(vec3 base, vec3 overlay, float strength) {
    return base + overlay * strength;
}

vec3 multiplyBlend(vec3 base, vec3 overlay, float strength) {
    return mix(base, base * overlay, strength);
}

vec3 screenBlend(vec3 base, vec3 overlay, float strength) {
    vec3 result = 1.0 - (1.0 - base) * (1.0 - overlay);
    return mix(base, result, strength);
}

vec3 overlayBlend(vec3 base, vec3 overlay, float strength) {
    vec3 result;
    for(int i = 0; i < 3; i++) {
        if(base[i] < 0.5) {
            result[i] = 2.0 * base[i] * overlay[i];
        } else {
            result[i] = 1.0 - 2.0 * (1.0 - base[i]) * (1.0 - overlay[i]);
        }
    }
    return mix(base, result, strength);
}

vec3 colorBurnBlend(vec3 base, vec3 overlay, float strength) {
    vec3 result = 1.0 - (1.0 - base) / (overlay + 0.001);
    return mix(base, clamp(result, 0.0, 1.0), strength);
}

// 単一レイヤーを描画する関数
float drawLayer(vec2 P, float layerTime, float baseRadius, float innerSpeed, float outerSpeed, float circleSize, float lineThickness) {
    float v = 0.0;
    
    // レイヤー用の circle 配列を初期化
    C[0] = vec2(0.0); // center
    
    // 時間に基づく拡張係数（波のように拡張・収縮）
    float expansionWave = sin(layerTime * 0.5) * 0.5 + 1.0; // 0.5 - 1.5の範囲
    float innerExpansion = sin(layerTime * 0.3) * 0.3 + 0.8; // 0.5 - 1.1の範囲
    float outerExpansion = sin(layerTime * 0.2) * 0.4 + 1.2; // 0.8 - 1.6の範囲
    
    vec3 s = 1. + fract( layerTime + vec3(0,1,2)/3. );
    vec3 A = smoothstep(.5, -.3, abs(s-1.5) );
          
    // 内側の円（6個）- 時間と共に半径が拡張
    for(int i = 0; i < 6; i++) {
        float angle = float(i) * PI/3. + layerTime * innerSpeed;
        float dynamicRadius = baseRadius * innerExpansion;
        C[i+1] = dynamicRadius * vec2(cos(angle), sin(angle)) / s.y;
    }
    
    // 外側の円（6個）- より大きく拡張
    for(int i = 0; i < 6; i++) {
        float angle = float(i) * PI/3. + PI/6. + layerTime * outerSpeed;
        float dynamicRadius = 2.0 * baseRadius * outerExpansion;
        C[i+7] = dynamicRadius * vec2(cos(angle), sin(angle)) / s.z;
    }

    // 中心の円
    v += drawCircle( P - C[0], circleSize ) * A.x;
    
    // 内側の円と線
    for(int j, i = 1; i < 7; i++) {
        v += drawCircle( P - C[i], circleSize ) * A.y
          +  drawLine(P, 0, i, lineThickness) * min(A.x,A.y);
        for( j = i + 1; j < 7; j++) 
            v += drawLine(P, i, j, lineThickness) * A.y;
        for(; j < 13; j++) 
            v += drawLine(P, i, j, lineThickness) * min(A.y,A.z);
    }
    
    // 外側の円と線
    for(int j, i = 7; i < 13; i++) {
        v += drawCircle(P-C[i], circleSize) * A.z;   
        for( j = i + 1; j < 13; j++) 
            v += drawLine(P, i, j, lineThickness) * A.z;
    }
    
    return v;
}

void mainImage(out vec4 O, vec2 u) {
    vec2 R = iResolution.xy,
         P = (u - .5*R ) / min(R.x, R.y);
    
    float t = iTime * .3;
    
    // レイヤー1: 基本レイヤー（拡張アニメーション）
    float layer1 = drawLayer(P, t, 0.1, 0.3, -0.2, 0.08, 0.002);
    vec3 color1 = hs(t*.1, .8) + hs(t*.1 + .33, .9);
    
    // レイヤー2: 速い拡張
    float layer2 = drawLayer(P, t * 1.8, 0.08, 0.5, -0.3, 0.06, 0.0015);
    vec3 color2 = hs(t*.15 + .2, .7) + hs(t*.15 + .5, .8);
    
    // レイヤー3: 遅い拡張、大きめ
    float layer3 = drawLayer(P * 0.7, t * 0.6, 0.12, 0.2, -0.15, 0.1, 0.0025);
    vec3 color3 = hs(t*.08 + .4, .6) + hs(t*.08 + .7, .7);
    
    // レイヤー4: 中速拡張
    float layer4 = drawLayer(P * 1.2, t * 1.2, 0.06, 0.4, -0.25, 0.05, 0.001);
    vec3 color4 = hs(t*.12 + .6, .9) + hs(t*.12 + .9, .8);
    
    // レイヤー5: 非常にゆっくりとした大きな拡張
    float layer5 = drawLayer(P * 0.5, t * 0.3, 0.15, 0.1, -0.08, 0.12, 0.003);
    vec3 color5 = hs(t*.05, .5) + hs(t*.05 + .8, .6);

    // 各レイヤーに異なるブレンドモードを適用
    vec3 baseColor = vec3(.05, .07, .1);
    vec3 result = baseColor;
    
    // レイヤー1: ソフト加算（基本の明るい効果）
    result = softAdditiveBlend(result, layer1 * color1, 0.2);
    
    // レイヤー2: スクリーン（明るく柔らかい重なり）
    result = screenBlend(result, layer2 * color2, 0.2);
    
    // レイヤー3: 乗算（深みのある重なり）
    result = multiplyBlend(result, layer3 * color3 + vec3(0.5), 0.25);
    
    // レイヤー4: オーバーレイ（コントラストのある重なり）
    result = overlayBlend(result, layer4 * color4, 0.35);
    
    // レイヤー5: 焼き込み（深みのある暗部効果）
    result = colorBurnBlend(result, layer5 * color5, 0.2);
    
    // 全体の明度を適度に制限
    result = clamp(result, vec3(0.0), vec3(0.95));
    
    O.rgb = result;
}