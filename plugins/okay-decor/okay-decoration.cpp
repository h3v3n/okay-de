/**
 * OKay-DE Native Window Decoration Engine for Wayfire 0.11+
 * Replaces Wayfire's built-in libdecoration.so with a modern, theme-aware
 * Cairo + PangoCairo server-side decoration engine supporting:
 *   - Dark & Light themes (live-synced with ~/.config/okay-de/panel.json)
 *   - Custom accent color borders & highlights
 *   - Rounded top corners (configurable radius)
 *   - Crisp anti-aliased title typography without text-shadow halos
 *   - 4 interactive button styles:
 *       * rectangles    (OKay rounded pills on right)
 *       * circles_left  (macOS/GNOME traffic lights on left with vector icons)
 *       * circles_right (Traffic lights on right with vector icons)
 *       * windows       (Windows 11 edge-to-edge fluent buttons on right)
 */

#include <wayfire/per-output-plugin.hpp>
#include <wayfire/plugin.hpp>
#include <wayfire/view.hpp>
#include <wayfire/toplevel.hpp>
#include <wayfire/toplevel-view.hpp>
#include <wayfire/window-manager.hpp>
#include <wayfire/workarea.hpp>
#include <wayfire/output.hpp>
#include <wayfire/core.hpp>
#include <wayfire/seat.hpp>
#include <wayfire/matcher.hpp>
#include <wayfire/option-wrapper.hpp>
#include <wayfire/signal-definitions.hpp>
#include <wayfire/scene.hpp>
#include <wayfire/scene-input.hpp>
#include <wayfire/scene-render.hpp>
#include <wayfire/scene-operations.hpp>
#include <wayfire/render-manager.hpp>
#include <wayfire/txn/transaction-manager.hpp>
#include <wayfire/plugins/common/cairo-util.hpp>
#include <wayfire/util.hpp>

#include <cairo.h>
#include <pango/pangocairo.h>
#include <linux/input-event-codes.h>
#include <sys/stat.h>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <memory>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

extern "C"
{
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/util/edges.h>
}

namespace wf
{
namespace okay_decor
{

enum class button_style_t
{
    RECTANGLES,
    CIRCLES_LEFT,
    CIRCLES_RIGHT,
    WINDOWS
};

enum class button_action_t
{
    NONE = 0,
    MINIMIZE,
    MAXIMIZE,
    CLOSE
};

struct rgb_t
{
    double r = 0.80;
    double g = 0.65;
    double b = 0.97;
    double a = 1.00;
};

struct okay_theme_config_t
{
    bool dark_mode = true;
    rgb_t accent = {0.796, 0.651, 0.969, 1.0}; // #cba6f7
    button_style_t button_style = button_style_t::RECTANGLES;
    int corner_radius = 12;
    time_t last_mtime = 0;

    static rgb_t parse_hex_color(const std::string& hex, rgb_t fallback)
    {
        std::string s = hex;
        auto pos = s.find('#');
        if (pos != std::string::npos)
        {
            s = s.substr(pos + 1);
        }
        if (s.size() < 6)
        {
            return fallback;
        }
        try
        {
            unsigned int val = std::stoul(s.substr(0, 6), nullptr, 16);
            rgb_t out;
            out.r = ((val >> 16) & 0xFF) / 255.0;
            out.g = ((val >> 8) & 0xFF) / 255.0;
            out.b = (val & 0xFF) / 255.0;
            out.a = 1.0;
            return out;
        } catch (...)
        {
            return fallback;
        }
    }

    static std::string extract_json_string(const std::string& content, const std::string& key)
    {
        std::string needle = "\"" + key + "\"";
        auto kpos = content.find(needle);
        if (kpos == std::string::npos)
        {
            return "";
        }
        auto cpos = content.find(':', kpos + needle.size());
        if (cpos == std::string::npos)
        {
            return "";
        }
        auto q1 = content.find('"', cpos + 1);
        if (q1 == std::string::npos)
        {
            return "";
        }
        auto q2 = content.find('"', q1 + 1);
        if (q2 == std::string::npos)
        {
            return "";
        }
        return content.substr(q1 + 1, q2 - q1 - 1);
    }

    static int extract_json_int(const std::string& content, const std::string& key, int fallback)
    {
        std::string needle = "\"" + key + "\"";
        auto kpos = content.find(needle);
        if (kpos == std::string::npos)
        {
            return fallback;
        }
        auto cpos = content.find(':', kpos + needle.size());
        if (cpos == std::string::npos)
        {
            return fallback;
        }
        size_t idx = cpos + 1;
        while (idx < content.size() && (content[idx] == ' ' || content[idx] == '\t' || content[idx] == '\n'))
        {
            idx++;
        }
        size_t end = idx;
        while (end < content.size() && (std::isdigit(static_cast<unsigned char>(content[end])) || content[end] == '-'))
        {
            end++;
        }
        if (end == idx)
        {
            return fallback;
        }
        try
        {
            return std::stoi(content.substr(idx, end - idx));
        } catch (...)
        {
            return fallback;
        }
    }

    bool reload_if_needed(bool force = false)
    {
        const char *home = std::getenv("HOME");
        if (!home)
        {
            return false;
        }
        std::string path = std::string(home) + "/.config/okay-de/panel.json";
        struct stat st{};
        if (stat(path.c_str(), &st) != 0)
        {
            return false;
        }
        if (!force && st.st_mtime == last_mtime)
        {
            return false;
        }
        last_mtime = st.st_mtime;

        std::ifstream ifs(path);
        if (!ifs.is_open())
        {
            return false;
        }
        std::stringstream buffer;
        buffer << ifs.rdbuf();
        std::string content = buffer.str();

        std::string mode = extract_json_string(content, "theme_mode");
        bool new_dark = (mode != "light");

        std::string acc = extract_json_string(content, "accent_color");
        rgb_t new_accent = parse_hex_color(acc, {0.796, 0.651, 0.969, 1.0});

        std::string bstyle_str = extract_json_string(content, "window_buttons_style");
        button_style_t new_style = button_style_t::RECTANGLES;
        if (bstyle_str == "circles_left")
        {
            new_style = button_style_t::CIRCLES_LEFT;
        } else if (bstyle_str == "circles_right")
        {
            new_style = button_style_t::CIRCLES_RIGHT;
        } else if (bstyle_str == "windows")
        {
            new_style = button_style_t::WINDOWS;
        } else
        {
            new_style = button_style_t::RECTANGLES;
        }

        int new_radius = std::clamp(extract_json_int(content, "window_corner_radius", 12), 0, 22);

        bool changed = (new_dark != dark_mode) ||
            (std::abs(new_accent.r - accent.r) > 0.002) ||
            (std::abs(new_accent.g - accent.g) > 0.002) ||
            (std::abs(new_accent.b - accent.b) > 0.002) ||
            (new_style != button_style) ||
            (new_radius != corner_radius);

        dark_mode = new_dark;
        accent = new_accent;
        button_style = new_style;
        corner_radius = new_radius;
        return changed || force;
    }
};

static okay_theme_config_t& get_theme_config()
{
    static okay_theme_config_t instance;
    return instance;
}

struct button_rect_t
{
    button_action_t action;
    wf::geometryf_t geom;
    bool hovered = false;
    bool pressed = false;
};

static void cairo_rounded_box(cairo_t *cr, double x, double y, double w, double h,
    double r_tl, double r_tr, double r_br, double r_bl)
{
    const double deg = M_PI / 180.0;
    cairo_new_sub_path(cr);
    cairo_arc(cr, x + w - r_tr, y + r_tr, r_tr, -90 * deg, 0 * deg);
    cairo_arc(cr, x + w - r_br, y + h - r_br, r_br, 0 * deg, 90 * deg);
    cairo_arc(cr, x + r_bl, y + h - r_bl, r_bl, 90 * deg, 180 * deg);
    cairo_arc(cr, x + r_tl, y + r_tl, r_tl, 180 * deg, 270 * deg);
    cairo_close_path(cr);
}

class okay_decoration_node_t : public wf::scene::node_t,
                               public wf::pointer_interaction_t,
                               public wf::touch_interaction_t
{
  public:
    wayfire_toplevel_view view;

    wf::option_wrapper_t<int> opt_title_height{"decoration/title_height"};
    wf::option_wrapper_t<int> opt_border_size{"decoration/border_size"};
    wf::option_wrapper_t<std::string> opt_font{"decoration/font"};
    wf::option_wrapper_t<wf::color_t> opt_active_color{"decoration/active_color"};
    wf::option_wrapper_t<wf::color_t> opt_inactive_color{"decoration/inactive_color"};
    wf::option_wrapper_t<std::string> opt_button_order{"decoration/button_order"};

    int width = 100;
    int height = 100;
    int current_titlebar = 34;
    int current_border = 3;

    std::vector<button_rect_t> buttons;
    wf::regionf_t cached_region;
    std::optional<wf::pointf_t> cursor_pos;
    int pressed_button_idx = -1;
    uint32_t last_title_click_ms = 0;

    std::shared_ptr<wf::texture_t> frame_tex;
    bool tex_dirty = true;

    wf::signal::connection_t<wf::view_title_changed_signal> on_title_changed =
        [=] (wf::view_title_changed_signal *)
    {
        invalidate_texture();
    };

    wf::signal::connection_t<wf::view_activated_state_signal> on_activated =
        [=] (wf::view_activated_state_signal *)
    {
        invalidate_texture();
    };

    wf::signal::connection_t<wf::view_geometry_changed_signal> on_geom_changed =
        [=] (wf::view_geometry_changed_signal *)
    {
        if (view)
        {
            auto dim = wf::dimensions(view->toplevel()->pending().geometry);
            if (dim.width > 0 && dim.height > 0)
            {
                resize_decoration(dim.width, dim.height);
            }
        }
    };

    okay_decoration_node_t(wayfire_toplevel_view v) : node_t(false), view(v)
    {
        get_theme_config().reload_if_needed(true);
        v->connect(&on_title_changed);
        v->connect(&on_activated);
        v->connect(&on_geom_changed);

        auto reload_cb = [=] ()
        {
            get_theme_config().reload_if_needed(true);
            rebuild_layout();
            invalidate_texture();
        };
        opt_title_height.set_callback(reload_cb);
        opt_border_size.set_callback(reload_cb);
        opt_font.set_callback(reload_cb);
        opt_active_color.set_callback(reload_cb);
        opt_inactive_color.set_callback(reload_cb);
        opt_button_order.set_callback(reload_cb);

        update_thickness();
    }

    void update_thickness()
    {
        int th = std::clamp(static_cast<int>(opt_title_height), 28, 52);
        int bs = std::clamp(static_cast<int>(opt_border_size), 1, 12);
        current_titlebar = th + bs;
        current_border = bs;
    }

    void invalidate_texture()
    {
        tex_dirty = true;
        wf::scene::damage_node(shared_from_this(), get_bounding_box());
    }

    void rebuild_layout()
    {
        update_thickness();
        buttons.clear();

        auto& cfg = get_theme_config();
        cfg.reload_if_needed(false);

        double th = current_titlebar;
        double bs = current_border;

        if (cfg.button_style == button_style_t::CIRCLES_LEFT)
        {
            double btn_size = 18.0;
            double gap = 8.0;
            double start_x = bs + 12.0;
            double y = bs + std::floor((th - bs - btn_size) * 0.5);

            buttons.push_back({button_action_t::CLOSE, {start_x, y, btn_size, btn_size}});
            buttons.push_back({button_action_t::MINIMIZE, {start_x + (btn_size + gap), y, btn_size, btn_size}});
            buttons.push_back({button_action_t::MAXIMIZE, {start_x + 2.0 * (btn_size + gap), y, btn_size, btn_size}});
        } else if (cfg.button_style == button_style_t::CIRCLES_RIGHT)
        {
            double btn_size = 18.0;
            double gap = 8.0;
            double total_w = 3.0 * btn_size + 2.0 * gap;
            double start_x = std::max(bs + 8.0, static_cast<double>(width) - bs - 12.0 - total_w);
            double y = bs + std::floor((th - bs - btn_size) * 0.5);

            buttons.push_back({button_action_t::MINIMIZE, {start_x, y, btn_size, btn_size}});
            buttons.push_back({button_action_t::MAXIMIZE, {start_x + (btn_size + gap), y, btn_size, btn_size}});
            buttons.push_back({button_action_t::CLOSE, {start_x + 2.0 * (btn_size + gap), y, btn_size, btn_size}});
        } else if (cfg.button_style == button_style_t::WINDOWS)
        {
            double btn_w = 40.0;
            double btn_h = std::max(22.0, th - bs - 2.0);
            double total_w = 3.0 * btn_w;
            double start_x = std::max(bs + 4.0, static_cast<double>(width) - bs - 2.0 - total_w);
            double y = bs + 1.0;

            buttons.push_back({button_action_t::MINIMIZE, {start_x, y, btn_w, btn_h}});
            buttons.push_back({button_action_t::MAXIMIZE, {start_x + btn_w, y, btn_w, btn_h}});
            buttons.push_back({button_action_t::CLOSE, {start_x + 2.0 * btn_w, y, btn_w, btn_h}});
        } else
        {
            // RECTANGLES (OKay Modern Rounded Pills)
            double btn_w = 28.0;
            double btn_h = 22.0;
            double gap = 6.0;
            double total_w = 3.0 * btn_w + 2.0 * gap;
            double start_x = std::max(bs + 8.0, static_cast<double>(width) - bs - 10.0 - total_w);
            double y = bs + std::floor((th - bs - btn_h) * 0.5);

            buttons.push_back({button_action_t::MINIMIZE, {start_x, y, btn_w, btn_h}});
            buttons.push_back({button_action_t::MAXIMIZE, {start_x + (btn_w + gap), y, btn_w, btn_h}});
            buttons.push_back({button_action_t::CLOSE, {start_x + 2.0 * (btn_w + gap), y, btn_w, btn_h}});
        }

        cached_region.clear();
        if (width > 0 && height > 0)
        {
            double dw = width;
            double dh = height;
            // Top titlebar + left, right, bottom borders
            cached_region |= wf::geometryf_t{0.0, 0.0, dw, th};
            cached_region |= wf::geometryf_t{0.0, th, bs, std::max(0.0, dh - th)};
            cached_region |= wf::geometryf_t{std::max(0.0, dw - bs), th, bs, std::max(0.0, dh - th)};
            cached_region |= wf::geometryf_t{0.0, std::max(0.0, dh - bs), dw, bs};
        }
    }

    void resize_decoration(int w, int h)
    {
        if (w <= 0 || h <= 0)
        {
            return;
        }
        if (width == w && height == h && !tex_dirty)
        {
            return;
        }
        wf::scene::damage_node(shared_from_this(), get_bounding_box());
        width = w;
        height = h;
        rebuild_layout();
        tex_dirty = true;
        wf::scene::damage_node(shared_from_this(), get_bounding_box());
    }

    void draw_button_glyph(cairo_t *cr, button_action_t action, double cx, double cy,
        double size, rgb_t color, double line_w)
    {
        cairo_set_source_rgba(cr, color.r, color.g, color.b, color.a);
        cairo_set_line_width(cr, line_w);
        cairo_set_line_cap(cr, CAIRO_LINE_CAP_ROUND);
        cairo_set_line_join(cr, CAIRO_LINE_JOIN_ROUND);

        double hs = size * 0.5;
        if (action == button_action_t::CLOSE)
        {
            cairo_move_to(cr, cx - hs, cy - hs);
            cairo_line_to(cr, cx + hs, cy + hs);
            cairo_move_to(cr, cx + hs, cy - hs);
            cairo_line_to(cr, cx - hs, cy + hs);
            cairo_stroke(cr);
        } else if (action == button_action_t::MAXIMIZE)
        {
            bool is_tiled = view && (view->pending_tiled_edges() != 0);
            if (is_tiled)
            {
                // Restore icon (two overlapping squares)
                double s = size * 0.78;
                cairo_rectangle(cr, cx - hs, cy - hs + (size - s), s, s);
                cairo_stroke(cr);
                cairo_move_to(cr, cx - hs + (size - s), cy - hs + (size - s));
                cairo_line_to(cr, cx - hs + (size - s), cy - hs);
                cairo_line_to(cr, cx + hs, cy - hs);
                cairo_line_to(cr, cx + hs, cy + hs - (size - s));
                cairo_line_to(cr, cx - hs + s, cy + hs - (size - s));
                cairo_stroke(cr);
            } else
            {
                cairo_rectangle(cr, cx - hs, cy - hs, size, size);
                cairo_stroke(cr);
            }
        } else if (action == button_action_t::MINIMIZE)
        {
            cairo_move_to(cr, cx - hs, cy + 0.5);
            cairo_line_to(cr, cx + hs, cy + 0.5);
            cairo_stroke(cr);
        }
    }

    void ensure_frame_texture()
    {
        bool cfg_changed = get_theme_config().reload_if_needed(false);
        if (cfg_changed)
        {
            rebuild_layout();
            tex_dirty = true;
        }
        if (!tex_dirty && frame_tex)
        {
            return;
        }
        if (width <= 0 || height <= 0)
        {
            return;
        }

        auto& cfg = get_theme_config();
        bool active = view ? view->activated : true;
        bool tiled = view ? (view->pending_tiled_edges() != 0) : false;
        double radius = tiled ? 0.0 : static_cast<double>(cfg.corner_radius);

        cairo_surface_t *surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, width, height);
        cairo_t *cr = cairo_create(surface);
        cairo_set_antialias(cr, CAIRO_ANTIALIAS_BEST);

        // 1. Clear surface to transparent
        cairo_set_operator(cr, CAIRO_OPERATOR_CLEAR);
        cairo_paint(cr);
        cairo_set_operator(cr, CAIRO_OPERATOR_OVER);

        // Theme palette
        rgb_t bg_top, bg_bot, border_col, text_col;
        if (cfg.dark_mode)
        {
            if (active)
            {
                bg_top = {0.145, 0.145, 0.220, 0.98}; // #252538
                bg_bot = {0.118, 0.118, 0.180, 0.98}; // #1e1e2e
                border_col = {cfg.accent.r, cfg.accent.g, cfg.accent.b, 0.80};
                text_col = {0.804, 0.839, 0.957, 1.0}; // #cdd6f4
            } else
            {
                bg_top = {0.102, 0.102, 0.153, 0.96}; // #1a1a27
                bg_bot = {0.094, 0.094, 0.145, 0.96}; // #181825
                border_col = {0.192, 0.196, 0.267, 0.85}; // #313244
                text_col = {0.576, 0.600, 0.698, 0.88}; // #9399b2
            }
        } else
        {
            if (active)
            {
                bg_top = {0.965, 0.973, 0.988, 0.98}; // #f6f8fc
                bg_bot = {0.937, 0.945, 0.961, 0.98}; // #eff1f5
                border_col = {cfg.accent.r, cfg.accent.g, cfg.accent.b, 0.85};
                text_col = {0.298, 0.310, 0.412, 1.0}; // #4c4f69
            } else
            {
                bg_top = {0.918, 0.929, 0.949, 0.96}; // #eaedf2
                bg_bot = {0.902, 0.914, 0.937, 0.96}; // #e6e9ef
                border_col = {0.737, 0.753, 0.800, 0.85}; // #bcc0cc
                text_col = {0.424, 0.435, 0.522, 0.90}; // #6c6f85
            }
        }

        // 2. Clip out inner client area so we only draw on the titlebar + borders
        cairo_save(cr);
        cairo_rectangle(cr, 0, 0, width, height);
        int inner_w = std::max(0, width - 2 * current_border);
        int inner_h = std::max(0, height - current_titlebar - current_border);
        if (inner_w > 0 && inner_h > 0)
        {
            cairo_rectangle(cr, current_border, current_titlebar, inner_w, inner_h);
        }
        cairo_set_fill_rule(cr, CAIRO_FILL_RULE_EVEN_ODD);
        cairo_clip(cr);
        cairo_set_fill_rule(cr, CAIRO_FILL_RULE_WINDING);

        // 3. Draw rounded outer frame background
        double r_bot = std::min(radius * 0.5, static_cast<double>(current_border + 2));
        cairo_rounded_box(cr, 0.5, 0.5, width - 1.0, height - 1.0, radius, radius, r_bot, r_bot);

        cairo_pattern_t *grad = cairo_pattern_create_linear(0, 0, 0, current_titlebar);
        cairo_pattern_add_color_stop_rgba(grad, 0.0, bg_top.r, bg_top.g, bg_top.b, bg_top.a);
        cairo_pattern_add_color_stop_rgba(grad, 1.0, bg_bot.r, bg_bot.g, bg_bot.b, bg_bot.a);
        cairo_set_source(cr, grad);
        cairo_fill_preserve(cr);
        cairo_pattern_destroy(grad);

        // Subtle separator line at bottom of titlebar
        if (cfg.dark_mode)
        {
            cairo_set_source_rgba(cr, 1.0, 1.0, 1.0, active ? 0.06 : 0.03);
        } else
        {
            cairo_set_source_rgba(cr, 0.0, 0.0, 0.0, active ? 0.07 : 0.04);
        }
        cairo_set_line_width(cr, 1.0);
        cairo_move_to(cr, current_border, current_titlebar - 0.5);
        cairo_line_to(cr, width - current_border, current_titlebar - 0.5);
        cairo_stroke(cr);

        // Outer accent/frame border stroke
        cairo_rounded_box(cr, 0.75, 0.75, width - 1.5, height - 1.5, radius, radius, r_bot, r_bot);
        cairo_set_source_rgba(cr, border_col.r, border_col.g, border_col.b, border_col.a);
        cairo_set_line_width(cr, active ? 1.6 : 1.1);
        cairo_stroke(cr);

        // 4. Render Window Title (crisp Pango layout, zero text-shadow/outline!)
        std::string title = view ? view->get_title() : "";
        if (!title.empty())
        {
            int left_pad = current_border + 14;
            int right_pad = current_border + 14;
            if (cfg.button_style == button_style_t::CIRCLES_LEFT)
            {
                left_pad = current_border + 96;
            } else if (cfg.button_style == button_style_t::WINDOWS)
            {
                right_pad = current_border + 132;
            } else
            {
                right_pad = current_border + 114;
            }

            int avail_w = std::max(0, width - left_pad - right_pad);
            if (avail_w > 30)
            {
                PangoLayout *layout = pango_cairo_create_layout(cr);
                std::string font_str = "Inter Semi-Bold 10.5";
                std::string user_font = opt_font;
                if (!user_font.empty() && user_font.find("Mono") == std::string::npos)
                {
                    font_str = user_font + " Semi-Bold 10.5";
                }
                PangoFontDescription *desc = pango_font_description_from_string(font_str.c_str());
                pango_layout_set_font_description(layout, desc);
                pango_layout_set_text(layout, title.c_str(), -1);
                pango_layout_set_single_paragraph_mode(layout, TRUE);
                pango_layout_set_ellipsize(layout, PANGO_ELLIPSIZE_END);
                pango_layout_set_width(layout, avail_w * PANGO_SCALE);

                int text_w = 0, text_h = 0;
                pango_layout_get_pixel_size(layout, &text_w, &text_h);

                // Center title horizontally when possible, clamped to safe bounds
                int tx = (width - text_w) / 2;
                if (tx < left_pad)
                {
                    tx = left_pad;
                }
                if (tx + text_w > width - right_pad)
                {
                    tx = std::max(left_pad, width - right_pad - text_w);
                }
                int ty = current_border + (current_titlebar - current_border - text_h) / 2;

                cairo_set_source_rgba(cr, text_col.r, text_col.g, text_col.b, text_col.a);
                cairo_move_to(cr, tx, ty);
                pango_cairo_show_layout(cr, layout);

                pango_font_description_free(desc);
                g_object_unref(layout);
            }
        }

        // 5. Render Window Buttons according to selected button_style
        for (const auto& btn : buttons)
        {
            double bx = btn.geom.x;
            double by = btn.geom.y;
            double bw = btn.geom.width;
            double bh = btn.geom.height;
            double cx = bx + bw * 0.5;
            double cy = by + bh * 0.5;

            if (cfg.button_style == button_style_t::CIRCLES_LEFT ||
                cfg.button_style == button_style_t::CIRCLES_RIGHT)
            {
                // Colored circles with crisp dark icons inside
                rgb_t c_fill;
                if (btn.action == button_action_t::CLOSE)
                {
                    c_fill = {0.953, 0.545, 0.659, 1.0}; // #f38ba8
                } else if (btn.action == button_action_t::MINIMIZE)
                {
                    c_fill = {0.976, 0.886, 0.686, 1.0}; // #f9e2af
                } else
                {
                    c_fill = {0.651, 0.890, 0.631, 1.0}; // #a6e3a1
                }

                double alpha = active ? (btn.hovered ? 1.0 : 0.90) : (btn.hovered ? 0.88 : 0.48);
                double r = std::min(bw, bh) * 0.44;
                if (btn.hovered)
                {
                    r = std::min(bw, bh) * 0.48;
                }

                cairo_arc(cr, cx, cy, r, 0, 2 * M_PI);
                cairo_set_source_rgba(cr, c_fill.r, c_fill.g, c_fill.b, alpha);
                cairo_fill(cr);

                // Crisp dark glyph inside the circle (always visible!)
                rgb_t glyph_col = {0.094, 0.094, 0.145, btn.hovered ? 0.95 : (active ? 0.78 : 0.55)};
                draw_button_glyph(cr, btn.action, cx, cy, 6.2, glyph_col, 1.5);
            } else if (cfg.button_style == button_style_t::WINDOWS)
            {
                // Windows 11 edge-to-edge button style
                if (btn.hovered || btn.pressed)
                {
                    cairo_rounded_box(cr, bx + 1, by + 1, bw - 2, bh - 2, 4, 4, 4, 4);
                    if (btn.action == button_action_t::CLOSE)
                    {
                        cairo_set_source_rgba(cr, 0.910, 0.067, 0.137, btn.pressed ? 0.85 : 0.95); // #e81123
                    } else if (cfg.dark_mode)
                    {
                        cairo_set_source_rgba(cr, 1.0, 1.0, 1.0, btn.pressed ? 0.16 : 0.10);
                    } else
                    {
                        cairo_set_source_rgba(cr, 0.0, 0.0, 0.0, btn.pressed ? 0.12 : 0.07);
                    }
                    cairo_fill(cr);
                }

                rgb_t glyph_col = text_col;
                if (btn.hovered && btn.action == button_action_t::CLOSE)
                {
                    glyph_col = {1.0, 1.0, 1.0, 1.0};
                }
                draw_button_glyph(cr, btn.action, cx, cy, 8.5, glyph_col, 1.35);
            } else
            {
                // RECTANGLES (OKay Modern Rounded Pills)
                cairo_rounded_box(cr, bx, by, bw, bh, 6.0, 6.0, 6.0, 6.0);
                if (btn.hovered || btn.pressed)
                {
                    if (btn.action == button_action_t::CLOSE)
                    {
                        cairo_set_source_rgba(cr, 0.953, 0.545, 0.659, btn.pressed ? 0.85 : 0.95); // #f38ba8
                    } else
                    {
                        cairo_set_source_rgba(cr, cfg.accent.r, cfg.accent.g, cfg.accent.b,
                            btn.pressed ? 0.35 : 0.25);
                    }
                } else
                {
                    if (cfg.dark_mode)
                    {
                        cairo_set_source_rgba(cr, 1.0, 1.0, 1.0, active ? 0.075 : 0.04);
                    } else
                    {
                        cairo_set_source_rgba(cr, 0.0, 0.0, 0.0, active ? 0.065 : 0.035);
                    }
                }
                cairo_fill(cr);

                rgb_t glyph_col = text_col;
                if (btn.hovered && btn.action == button_action_t::CLOSE)
                {
                    glyph_col = {0.094, 0.094, 0.145, 1.0};
                } else if (btn.hovered)
                {
                    glyph_col = cfg.dark_mode ? rgb_t{0.95, 0.96, 1.0, 1.0} : rgb_t{0.15, 0.16, 0.24, 1.0};
                }
                draw_button_glyph(cr, btn.action, cx, cy, 7.6, glyph_col, 1.55);
            }
        }

        cairo_restore(cr);
        cairo_surface_flush(surface);

        int stride = cairo_image_surface_get_stride(surface);
        unsigned char *data = cairo_image_surface_get_data(surface);
        wlr_texture *wlr_tex = wlr_texture_from_pixels(
            wf::get_core().renderer, DRM_FORMAT_ARGB8888, stride, width, height, data);
        if (wlr_tex)
        {
            frame_tex = wf::texture_t::from_texture(wlr_tex);
        }

        cairo_destroy(cr);
        cairo_surface_destroy(surface);
        tex_dirty = false;
    }

    uint32_t calculate_resize_edges(wf::pointf_t pt) const
    {
        uint32_t edges = 0;
        int corner_zone = std::max(14, current_border + 10);

        if (pt.x <= current_border || (pt.x <= corner_zone && (pt.y <= corner_zone || pt.y >= height - corner_zone)))
        {
            edges |= WLR_EDGE_LEFT;
        } else if (pt.x >= width - current_border ||
                   (pt.x >= width - corner_zone && (pt.y <= corner_zone || pt.y >= height - corner_zone)))
        {
            edges |= WLR_EDGE_RIGHT;
        }

        if (pt.y <= current_border)
        {
            edges |= WLR_EDGE_TOP;
        } else if (pt.y >= height - current_border)
        {
            edges |= WLR_EDGE_BOTTOM;
        }

        return edges;
    }

    int find_button_at(wf::pointf_t pt) const
    {
        for (size_t i = 0; i < buttons.size(); i++)
        {
            const auto& g = buttons[i].geom;
            if (pt.x >= g.x && pt.x < g.x + g.width &&
                pt.y >= g.y && pt.y < g.y + g.height)
            {
                return static_cast<int>(i);
            }
        }
        return -1;
    }

    void update_hover_and_cursor(wf::pointf_t pt)
    {
        int hovered_idx = find_button_at(pt);
        bool hover_changed = false;
        for (size_t i = 0; i < buttons.size(); i++)
        {
            bool h = (static_cast<int>(i) == hovered_idx);
            if (buttons[i].hovered != h)
            {
                buttons[i].hovered = h;
                hover_changed = true;
            }
        }
        if (hover_changed)
        {
            invalidate_texture();
        }

        if (hovered_idx >= 0)
        {
            wf::get_core().set_cursor("default");
            return;
        }

        uint32_t edges = calculate_resize_edges(pt);
        if (edges != 0)
        {
            wf::get_core().set_cursor(wlr_xcursor_get_resize_name(static_cast<wlr_edges>(edges)));
        } else
        {
            wf::get_core().set_cursor("default");
        }
    }

    void trigger_button_action(button_action_t action)
    {
        if (!view)
        {
            return;
        }
        if (action == button_action_t::CLOSE)
        {
            view->close();
        } else if (action == button_action_t::MAXIMIZE)
        {
            if (view->pending_tiled_edges())
            {
                wf::get_core().default_wm->tile_request(view, 0);
            } else
            {
                wf::get_core().default_wm->tile_request(view, wf::TILED_EDGES_ALL);
            }
        } else if (action == button_action_t::MINIMIZE)
        {
            wf::get_core().default_wm->minimize_request(view, true);
        }
    }

    // --- Pointer interaction ---
    void handle_pointer_enter(wf::pointf_t position) override
    {
        position -= wf::pointf_t{get_offset()};
        cursor_pos = position;
        update_hover_and_cursor(position);
    }

    void handle_pointer_leave() override
    {
        cursor_pos.reset();
        bool changed = false;
        for (auto& b : buttons)
        {
            if (b.hovered || b.pressed)
            {
                b.hovered = false;
                b.pressed = false;
                changed = true;
            }
        }
        pressed_button_idx = -1;
        if (changed)
        {
            invalidate_texture();
        }
    }

    void handle_pointer_motion(wf::pointf_t to, uint32_t) override
    {
        to -= wf::pointf_t{get_offset()};
        cursor_pos = to;
        update_hover_and_cursor(to);
    }

    void handle_pointer_button(const wlr_pointer_button_event& ev) override
    {
        if (ev.button != BTN_LEFT || !cursor_pos || !view)
        {
            return;
        }

        wf::pointf_t pt = *cursor_pos;
        if (ev.state == WL_POINTER_BUTTON_STATE_PRESSED)
        {
            wf::get_core().default_wm->focus_raise_view(view);

            int btn_idx = find_button_at(pt);
            if (btn_idx >= 0)
            {
                pressed_button_idx = btn_idx;
                buttons[btn_idx].pressed = true;
                invalidate_texture();
                return;
            }

            uint32_t edges = calculate_resize_edges(pt);
            if (edges != 0)
            {
                wf::get_core().default_wm->resize_request(view, edges);
                return;
            }

            // Titlebar double-click maximizes/restores, single-drag moves
            uint32_t now = wf::get_current_time();
            if (now - last_title_click_ms < 320)
            {
                last_title_click_ms = 0;
                trigger_button_action(button_action_t::MAXIMIZE);
                return;
            }
            last_title_click_ms = now;
            wf::get_core().default_wm->move_request(view);
        } else if (ev.state == WL_POINTER_BUTTON_STATE_RELEASED)
        {
            if (pressed_button_idx >= 0 && pressed_button_idx < static_cast<int>(buttons.size()))
            {
                int released_idx = find_button_at(pt);
                button_action_t act = buttons[pressed_button_idx].action;
                buttons[pressed_button_idx].pressed = false;
                bool matches = (released_idx == pressed_button_idx);
                pressed_button_idx = -1;
                invalidate_texture();
                if (matches)
                {
                    trigger_button_action(act);
                }
            }
        }
    }

    // --- Touch interaction ---
    void handle_touch_down(uint32_t time_ms, int finger_id, wf::pointf_t pos) override
    {
        if (finger_id != 0 || !view)
        {
            return;
        }
        pos -= wf::pointf_t{get_offset()};
        cursor_pos = pos;
        wf::get_core().default_wm->focus_raise_view(view);

        int btn_idx = find_button_at(pos);
        if (btn_idx >= 0)
        {
            pressed_button_idx = btn_idx;
            buttons[btn_idx].pressed = true;
            invalidate_texture();
            return;
        }

        uint32_t edges = calculate_resize_edges(pos);
        if (edges != 0)
        {
            wf::get_core().default_wm->resize_request(view, edges);
        } else
        {
            wf::get_core().default_wm->move_request(view);
        }
        (void)time_ms;
    }

    void handle_touch_up(uint32_t, int finger_id, wf::pointf_t lift_off_position) override
    {
        if (finger_id != 0)
        {
            return;
        }
        lift_off_position -= wf::pointf_t{get_offset()};
        if (pressed_button_idx >= 0 && pressed_button_idx < static_cast<int>(buttons.size()))
        {
            int idx = find_button_at(lift_off_position);
            button_action_t act = buttons[pressed_button_idx].action;
            buttons[pressed_button_idx].pressed = false;
            bool matches = (idx == pressed_button_idx);
            pressed_button_idx = -1;
            invalidate_texture();
            if (matches)
            {
                trigger_button_action(act);
            }
        }
    }

    void handle_touch_motion(uint32_t, int finger_id, wf::pointf_t position) override
    {
        if (finger_id == 0)
        {
            position -= wf::pointf_t{get_offset()};
            cursor_pos = position;
        }
    }

    // --- Scene node interface ---
    wf::pointf_t get_offset() const
    {
        return {static_cast<double>(-current_border), static_cast<double>(-current_titlebar)};
    }

    std::string stringify() const override
    {
        return "okay-decoration";
    }

    wf::pointer_interaction_t& pointer_interaction() override
    {
        return *this;
    }

    wf::touch_interaction_t& touch_interaction() override
    {
        return *this;
    }

    std::optional<wf::scene::input_node_t> find_node_at(const wf::pointf_t& at) override
    {
        wf::pointf_t local = at - get_offset();
        if (cached_region.contains_pointf(local))
        {
            return wf::scene::input_node_t{
                .node = this,
                .local_coords = at,
            };
        }
        return {};
    }

    wf::geometryf_t get_bounding_box() override
    {
        return wf::construct_box(get_offset(), wf::dimensions_t{width, height});
    }

    class decoration_render_instance_t : public wf::scene::render_instance_t
    {
        okay_decoration_node_t *self;
        wf::scene::damage_callback push_damage;

        wf::signal::connection_t<wf::scene::node_damage_signal> on_damage =
            [=] (wf::scene::node_damage_signal *data)
        {
            push_damage(data->region);
        };

      public:
        decoration_render_instance_t(okay_decoration_node_t *s, wf::scene::damage_callback pd)
            : self(s), push_damage(pd)
        {
            self->connect(&on_damage);
        }

        void schedule_instructions(std::vector<wf::scene::render_instruction_t>& instructions,
            const wf::render_target_t& target, wf::regionf_t& damage) override
        {
            wf::regionf_t our_region = self->cached_region + self->get_offset();
            wf::regionf_t our_damage = damage & our_region;
            if (!our_damage.empty())
            {
                instructions.push_back(wf::scene::render_instruction_t{
                    .instance = this,
                    .target   = target,
                    .damage   = std::move(our_damage),
                });
            }
        }

        void render(const wf::scene::render_instruction_t& data) override
        {
            self->ensure_frame_texture();
            if (!self->frame_tex)
            {
                return;
            }
            auto bbox = self->get_bounding_box();
            data.pass->add_texture(self->frame_tex, data.target, bbox, data.damage, 1.0f);
        }
    };

    void gen_render_instances(std::vector<wf::scene::render_instance_uptr>& instances,
        wf::scene::damage_callback push_damage, wf::output_t *) override
    {
        instances.push_back(std::make_unique<decoration_render_instance_t>(this, push_damage));
    }
};

class simple_decorator_t : public wf::custom_data_t
{
    wayfire_toplevel_view view;
    std::shared_ptr<okay_decoration_node_t> deco;

    wf::signal::connection_t<wf::view_set_output_signal> on_set_output =
        [=] (wf::view_set_output_signal *)
    {
        if (deco)
        {
            deco->invalidate_texture();
        }
    };

  public:
    simple_decorator_t(wayfire_toplevel_view v) : view(v)
    {
        deco = std::make_shared<okay_decoration_node_t>(view);
        auto dim = wf::dimensions(view->toplevel()->pending().geometry);
        deco->resize_decoration(std::max(100, dim.width), std::max(60, dim.height));
        wf::scene::add_back(view->get_surface_root_node(), deco);
        view->connect(&on_set_output);
    }

    ~simple_decorator_t()
    {
        wf::scene::remove_child(deco);
    }

    wf::decoration_margins_t get_margins(const wf::toplevel_state_t& state)
    {
        if (state.fullscreen)
        {
            return {0.0, 0.0, 0.0, 0.0};
        }
        deco->update_thickness();
        return wf::decoration_margins_t{
            .left   = static_cast<double>(deco->current_border),
            .right  = static_cast<double>(deco->current_border),
            .bottom = static_cast<double>(deco->current_border),
            .top    = static_cast<double>(deco->current_titlebar),
        };
    }

    void refresh_theme()
    {
        if (deco)
        {
            deco->rebuild_layout();
            deco->invalidate_texture();
        }
    }
};

class wayfire_okay_decoration : public wf::plugin_interface_t
{
    wf::view_matcher_t ignore_views{"decoration/ignore_views"};
    wf::view_matcher_t forced_views{"decoration/forced_views"};
    wf::wl_timer<true> theme_watch_timer;

    wf::signal::connection_t<wf::txn::new_transaction_signal> on_new_tx =
        [=] (wf::txn::new_transaction_signal *ev)
    {
        for (const auto& obj : ev->tx->get_objects())
        {
            if (auto toplevel = std::dynamic_pointer_cast<wf::toplevel_t>(obj))
            {
                if (auto deco = toplevel->get_data<simple_decorator_t>())
                {
                    toplevel->pending().margins = deco->get_margins(toplevel->pending());
                    continue;
                }

                if (toplevel->current().mapped || !toplevel->pending().mapped)
                {
                    continue;
                }

                auto view = wf::find_view_for_toplevel(toplevel);
                if (view && should_decorate_view(view))
                {
                    adjust_new_decorations(view);
                }
            }
        }
    };

    wf::signal::connection_t<wf::view_decoration_state_updated_signal> on_decoration_state_changed =
        [=] (wf::view_decoration_state_updated_signal *ev)
    {
        update_view_decoration(ev->view);
    };

    wf::signal::connection_t<wf::view_tiled_signal> on_view_tiled =
        [=] (wf::view_tiled_signal *ev)
    {
        update_view_decoration(ev->view);
    };

    wf::signal::connection_t<wf::reload_config_signal> on_reload_config =
        [=] (wf::reload_config_signal *)
    {
        get_theme_config().reload_if_needed(true);
        for (auto& view : wf::get_core().get_all_views())
        {
            update_view_decoration(view);
            if (auto toplevel = wf::toplevel_cast(view))
            {
                if (auto deco = toplevel->toplevel()->get_data<simple_decorator_t>())
                {
                    deco->refresh_theme();
                }
            }
        }
    };

  public:
    void init() override
    {
        get_theme_config().reload_if_needed(true);
        wf::get_core().connect(&on_decoration_state_changed);
        wf::get_core().connect(&on_view_tiled);
        wf::get_core().connect(&on_reload_config);
        wf::get_core().tx_manager->connect(&on_new_tx);

        theme_watch_timer.set_timeout(300, [=] () -> bool
        {
            if (get_theme_config().reload_if_needed(false))
            {
                for (auto& view : wf::get_core().get_all_views())
                {
                    if (auto toplevel = wf::toplevel_cast(view))
                    {
                        if (auto deco = toplevel->toplevel()->get_data<simple_decorator_t>())
                        {
                            deco->refresh_theme();
                        }
                    }
                }
            }
            return true;
        });

        for (auto& view : wf::get_core().get_all_views())
        {
            update_view_decoration(view);
        }
    }

    void fini() override
    {
        theme_watch_timer.disconnect();
        for (auto& view : wf::get_core().get_all_views())
        {
            if (auto toplevel = wf::toplevel_cast(view))
            {
                deinit_view(toplevel);
            }
        }
    }

    bool ignore_decoration_of_view(wayfire_view view)
    {
        return ignore_views.matches(view);
    }

    bool force_decoration_of_view(wayfire_view view)
    {
        return forced_views.matches(view);
    }

    bool should_decorate_view(wayfire_toplevel_view view)
    {
        return view->toplevel()->pending(). mapped &&
               !ignore_decoration_of_view(view) &&
               (force_decoration_of_view(view) || view->should_be_decorated());
    }

    void adjust_new_decorations(wayfire_toplevel_view view)
    {
        auto toplevel = view->toplevel();
        toplevel->store_data(std::make_unique<simple_decorator_t>(view));
        auto deco = toplevel->get_data<simple_decorator_t>();
        auto& pending = toplevel->pending();
        pending.margins = deco->get_margins(pending);
        if (!pending.fullscreen && !pending.tiled_edges)
        {
            pending.geometry = wf::expand_geometry_by_margins(pending.geometry, pending.margins);
            if (view->get_output())
            {
                pending.geometry = wf::clamp(pending.geometry, view->get_output()->workarea->get_workarea());
            }
        }
    }

    void deinit_view(wayfire_toplevel_view view)
    {
        auto deco = view->toplevel()->get_data<simple_decorator_t>();
        if (!deco)
        {
            return;
        }
        view->toplevel()->erase_data<simple_decorator_t>();
        auto& pending = view->toplevel()->pending();
        if (!pending.fullscreen && !pending.tiled_edges)
        {
            pending.geometry = wf::shrink_geometry_by_margins(pending.geometry, pending.margins);
        }
        pending.margins = {0, 0, 0, 0};
    }

    void update_view_decoration(wayfire_view view)
    {
        if (auto toplevel = wf::toplevel_cast(view))
        {
            bool want_decoration = should_decorate_view(toplevel);
            if (want_decoration != toplevel->toplevel()->has_data<simple_decorator_t>())
            {
                if (want_decoration)
                {
                    adjust_new_decorations(toplevel);
                } else
                {
                    deinit_view(toplevel);
                }
                wf::get_core().tx_manager->schedule_object(toplevel->toplevel());
            } else if (want_decoration)
            {
                if (auto deco = toplevel->toplevel()->get_data<simple_decorator_t>())
                {
                    deco->refresh_theme();
                }
            }
        }
    }
};

} // namespace okay_decor
} // namespace wf

DECLARE_WAYFIRE_PLUGIN(wf::okay_decor::wayfire_okay_decoration)
