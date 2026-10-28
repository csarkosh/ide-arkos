#include <metal_stdlib>
using namespace metal;

// The theme's colours, as float4 RGBA, by index.
constant int ARKOS_WINDOW_BG = 0;     constant int ARKOS_TITLE_BAR = 1;
constant int ARKOS_SIDEBAR = 2;       constant int ARKOS_TAB_BAR = 3;
constant int ARKOS_TAB_ACTIVE = 4;    constant int ARKOS_TAB_INACTIVE = 5;
constant int ARKOS_BORDER = 6;        constant int ARKOS_FOCUS_RING = 7;
constant int ARKOS_TEXT = 8;          constant int ARKOS_TEXT_DIM = 9;
constant int ARKOS_BANNER = 10;       constant int ARKOS_TERMINAL_BG = 11;
constant int ARKOS_TERMINAL_FG = 12;  constant int ARKOS_SELECTION = 13;
constant int ARKOS_WORKING = 14;      constant int ARKOS_WAITING = 15;
constant int ARKOS_BELL = 16;         constant int ARKOS_FIND_MATCH = 17;
constant int ARKOS_FIND_CURRENT = 18; constant int ARKOS_ANSI = 19; // 19..34: the sixteen
constant int ARKOS_COLORS = 35;

// What a block of the UI is.
constant int ARKOS_TITLE_BAR_BLOCK = 1;  constant int ARKOS_SIDEBAR_BLOCK = 2;
constant int ARKOS_DIVIDER_BLOCK = 3;    constant int ARKOS_TAB_BAR_BLOCK = 4;
constant int ARKOS_PANE_BODY_BLOCK = 5;  constant int ARKOS_ROW_HIGHLIGHT_BLOCK = 6;
constant int ARKOS_TAB_HIGHLIGHT_BLOCK = 7; constant int ARKOS_MEDIA_BLOCK = 8;
constant int ARKOS_TABLE_BLOCK = 9;      constant int ARKOS_TABLE_HEADER_BLOCK = 10;
// A media block is a video's picture, opaque over the wash: only what is drawn outside it shows.
// A table block is a markdown table, whose fills are clear; its header block is its first row.

// Which of a block's edges the app no longer draws (ink_borders): a bit each.
constant uint ARKOS_EDGE_TOP = 1;  constant uint ARKOS_EDGE_RIGHT = 2;
constant uint ARKOS_EDGE_BOTTOM = 4; constant uint ARKOS_EDGE_LEFT = 8;

struct ArkosBlock {
    float4 rect;      // x, y, width, height in window points, origin top-left
    float kind;       // ARKOS_*_BLOCK
    float edges;      // ARKOS_EDGE_* bits
    float seed;       // stable for the block's life: pick its grain from this, never from its index
    float value;      // a highlight's fade, 0 to 1 (1 for every other kind)
    float ordinal;    // a pane's place (0, 1, ...); a highlight's state (0 hovered, 1 selected, 2 both), else 0
    float3 pad;
};

struct ArkosInputs {
    float time;           // seconds since the effect started; frozen under Reduce Motion
    float intensity;      // the user's slider, 0 to 1 (config effect.intensity)
    float2 size;          // the window's content in points
    float scale;          // this drawable's pixels per point
    float block_count;
    float2 pad;
    float4 colors[35];
    ArkosBlock blocks[48];
};

struct ArkosVertex { float4 pos [[position]]; float2 uv; };
// The app supplies the vertex stage (a full-screen triangle). The shader supplies:
//   fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]);
