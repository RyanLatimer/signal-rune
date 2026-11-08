use crate::{Conversation, Delivery, Message, State};
use rune::{Color, Cx, El, Icon, ScrollSpec, TextStyle, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Kit, PaletteSeed, TextInput};

static CHAT: Icon = Icon {
    name: "chat",
    view: 24.,
    body: r#"<path d="M21 11.5a8.4 8.4 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.4 8.4 0 0 1-3.8-.9L3 21l1.9-5.7a8.4 8.4 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.4 8.4 0 0 1 3.8-.9h.5a8.5 8.5 0 0 1 8 8v.5Z" fill="none" stroke="currentColor" stroke-width="1.7"/>"#,
};
static EDIT: Icon = Icon {
    name: "edit",
    view: 24.,
    body: r#"<path d="M12 4H5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-7M16 3l5 5M10 14l-1 5 5-1L22 6l-5-5Z" fill="none" stroke="currentColor" stroke-width="1.6"/>"#,
};
static SEARCH: Icon = Icon {
    name: "search",
    view: 24.,
    body: r#"<circle cx="10.5" cy="10.5" r="6.5" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="m16 16 5 5" stroke="currentColor" stroke-width="1.7"/>"#,
};
static SETTINGS: Icon = Icon {
    name: "settings",
    view: 24.,
    body: r#"<circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="m10 2 4 0 1 3 3 1 3 3-1 3 0 3-3 3-3 1-1 3-4 0-1-3-3-1-3-3 1-3 0-3 3-3 3-1Z" fill="none" stroke="currentColor" stroke-width="1.4"/>"#,
};
static SEND: Icon = Icon {
    name: "send",
    view: 24.,
    body: r#"<path d="m4 4 17 8-17 8 3-8-3-8Zm3 8h14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>"#,
};
static LOCK: Icon = Icon {
    name: "lock",
    view: 24.,
    body: r#"<rect x="5" y="10" width="14" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M8 10V6a4 4 0 0 1 8 0v4" fill="none" stroke="currentColor" stroke-width="1.7"/>"#,
};
static BACK: Icon = Icon {
    name: "back",
    view: 24.,
    body: r#"<path d="m14 5-7 7 7 7" fill="none" stroke="currentColor" stroke-width="1.7"/>"#,
};
static PIN: Icon = Icon {
    name: "pin",
    view: 24.,
    body: r#"<path d="m9 3 6 0-1 7 4 4v2H6v-2l4-4-1-7Zm3 13v6" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>"#,
};
static ARCHIVE: Icon = Icon {
    name: "archive",
    view: 24.,
    body: r#"<path d="M4 8h16v12H4V8ZM3 3h18v5H3V3Zm6 9h6" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>"#,
};
fn typing_label(c: &Conversation) -> Option<String> {
    if c.typing.is_empty() {
        return None;
    }
    let mut names: Vec<_> = c.typing.values().map(|p| p.name.clone()).collect();
    names.sort();
    names.dedup();
    Some(match names.len() {
        1 => format!("{} is typing…", names[0]),
        2 => format!("{} and {} are typing…", names[0], names[1]),
        _ => format!("{} people are typing…", names.len()),
    })
}
fn color(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::BLACK)
}
struct Theme {
    bg: Color,
    sidebar: Color,
    rail: Color,
    fg: Color,
    muted: Color,
    line: Color,
    hover: Color,
    selected: Color,
    incoming: Color,
    blue: Color,
}
impl Theme {
    fn new(dark: bool) -> Self {
        let c = if dark {
            [
                "#202023", "#18191b", "#141416", "#f0f0f2", "#b3b3bb", "#343438", "#2c2c30",
                "#303e61", "#38383c",
            ]
        } else {
            [
                "#ffffff", "#fafafa", "#f0f0f0", "#1b1b1f", "#666670", "#e8e8ec", "#eeeeef",
                "#e5edff", "#f0f0f0",
            ]
        };
        Self {
            bg: color(c[0]),
            sidebar: color(c[1]),
            rail: color(c[2]),
            fg: color(c[3]),
            muted: color(c[4]),
            line: color(c[5]),
            hover: color(c[6]),
            selected: color(c[7]),
            incoming: color(c[8]),
            blue: color("#3269db"),
        }
    }
    fn kit(&self) -> Kit {
        Kit::from_palette(PaletteSeed {
            bg: self.bg,
            fg: self.fg,
            accent: self.blue,
        })
    }
}
fn label(s: impl Into<String>, size: f32, weight: f32, ink: Color) -> El {
    text(s).font(TextStyle::new(size, weight)).color(ink)
}
fn icon_button(
    key: &str,
    name: &str,
    glyph: &'static Icon,
    t: &Theme,
    f: impl Fn(&mut State) + 'static,
) -> El {
    el().key(key)
        .role("button")
        .label(name)
        .help(name)
        .focusable(true)
        .size(36., 36.)
        .radius(18.)
        .center()
        .hover(t.hover)
        .on_press(f)
        .child(icon(glyph, 20.).color(t.fg))
}
fn avatar(name: &str, size: f32, index: usize) -> El {
    let colors = [
        "#5266a6", "#9163a7", "#377e79", "#a56656", "#65784d", "#7563a7",
    ];
    el().size(size, size)
        .radius(size / 2.)
        .fill(color(colors[index % colors.len()]))
        .center()
        .a11y_hidden()
        .child(label(
            name.split_whitespace()
                .take(2)
                .filter_map(|n| n.chars().next())
                .collect::<String>(),
            size * 0.32,
            600.,
            Color::WHITE,
        ))
}
fn rail(s: &State, t: &Theme) -> El {
    col()
        .key("navigation")
        .w(64.)
        .h_full()
        .py(20.)
        .center_y()
        .gap(22.)
        .fill(t.rail)
        .child(avatar("You", 34., 0))
        .child(
            el().size(44., 40.)
                .radius(12.)
                .fill(t.selected)
                .center()
                .child(icon(&CHAT, 23.).color(t.fg)),
        )
        .child(el().grow(1.))
        .child(if s.demo {
            label("DEMO", 9., 600., t.muted)
        } else {
            el().size(7., 7.)
                .radius(4.)
                .fill(if s.bridge.is_some() && s.account.is_some() {
                    color("#64bd86")
                } else {
                    t.muted
                })
        })
        .child(icon_button("settings", "Settings", &SETTINGS, t, |s| {
            s.settings = !s.settings;
            s.mark_selected_read();
            s.persist();
        }))
}
fn sidebar(s: &State, t: &Theme, k: &Kit) -> El {
    let query = s.search.text.to_lowercase();
    let mut list = col().w_full().gap(2.);
    let mut count = 0;
    let mut conversations: Vec<_> = s
        .conversations
        .iter()
        .enumerate()
        .filter(|(_, c)| c.archived == s.show_archived)
        .collect();
    conversations.sort_by(|(_, a), (_, b)| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| {
                b.messages
                    .last()
                    .map(|m| m.timestamp)
                    .unwrap_or(0)
                    .cmp(&a.messages.last().map(|m| m.timestamp).unwrap_or(0))
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    for (i, c) in conversations {
        if !query.is_empty()
            && !c.name.to_lowercase().contains(&query)
            && !c.id.to_lowercase().contains(&query)
            && !c
                .messages
                .iter()
                .any(|m| m.body.to_lowercase().contains(&query))
        {
            continue;
        }
        count += 1;
        let selected = s.selected.as_ref() == Some(&c.id);
        let id = c.id.clone();
        let preview = typing_label(c).unwrap_or_else(|| {
            c.messages
                .last()
                .map(|m| {
                    format!(
                        "{}{}",
                        if m.outgoing { "You: " } else { "" },
                        m.body.replace('\n', " ")
                    )
                })
                .unwrap_or_else(|| "Start a conversation".into())
        });
        list = list.child(
            row()
                .key(format!("conversation-{i}"))
                .role("button")
                .label(&c.name)
                .focusable(true)
                .h(76.)
                .w_full()
                .px(12.)
                .gap(12.)
                .center_y()
                .radius(8.)
                .fill(if selected {
                    t.selected
                } else {
                    Color::TRANSPARENT
                })
                .hover(t.hover)
                .on_press_tx(move |s: &mut State, tx| {
                    s.choose(id.clone());
                    tx.scroll_to_bottom("messages");
                })
                .child(avatar(&c.name, 44., i))
                .child(
                    col()
                        .grow(1.)
                        .min_w(0.)
                        .gap(6.)
                        .child(
                            row()
                                .w_full()
                                .justify_between()
                                .child(
                                    row()
                                        .gap(4.)
                                        .center_y()
                                        .child(if c.pinned > 0 {
                                            icon(&PIN, 12.).color(t.muted)
                                        } else {
                                            el()
                                        })
                                        .child(
                                            label(&c.name, 14., 600., t.fg).max_w(155.).truncate(),
                                        ),
                                )
                                .child(label(
                                    c.messages
                                        .last()
                                        .map(|m| clock(m.timestamp))
                                        .unwrap_or_default(),
                                    10.,
                                    400.,
                                    t.muted,
                                )),
                        )
                        .child(label(preview, 12., 400., t.muted).truncate().max_w(204.)),
                )
                .child(if c.unread > 0 {
                    el().size(20., 20.)
                        .radius(10.)
                        .fill(t.blue)
                        .center()
                        .child(label(c.unread.to_string(), 10., 600., Color::WHITE))
                } else {
                    el()
                }),
        );
    }
    if count == 0 {
        list=list.child(col().padding(22.).gap(8.).child(label(if query.is_empty(){"Your conversations"}else{"No results"},14.,600.,t.fg)).child(label(if query.is_empty(){"Choose New message to get started. Contacts appear after your phone syncs."}else{"Try a different name or message."},12.,400.,t.muted).wrap()));
    }
    col()
        .key("sidebar")
        .w(312.)
        .h_full()
        .fill(t.sidebar)
        .px(8.)
        .gap(12.)
        .child(
            row()
                .h(64.)
                .px(12.)
                .center_y()
                .justify_between()
                .child(label(
                    if s.show_archived { "Archived" } else { "Chats" },
                    22.,
                    650.,
                    t.fg,
                ))
                .child(icon_button("new-message", "New message", &EDIT, t, |s| {
                    s.new_chat = true;
                    s.recipient.focused = true;
                })),
        )
        .child(
            TextInput::new(&s.search)
                .label("Search conversations")
                .placeholder("Search")
                .leading_icon(&SEARCH)
                .on_edit(|s: &mut State| &mut s.search)
                .build(k)
                .key("search")
                .mx(8.),
        )
        .child(
            col()
                .key("conversation-list")
                .grow(1.)
                .min_h(0.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        )
        .child(
            Button::new(if s.show_archived {
                "Back to chats".to_string()
            } else {
                format!(
                    "Archived ({})",
                    s.conversations.iter().filter(|c| c.archived).count()
                )
            })
            .variant(ButtonVariant::Ghost)
            .on_press(|s: &mut State| {
                s.show_archived = !s.show_archived;
                s.selected = None;
                s.dirty = true;
                s.persist();
            })
            .build(k)
            .key("archived-chats"),
        )
        .child(
            row()
                .h(38.)
                .center_y()
                .px(12.)
                .gap(6.)
                .child(icon(&LOCK, 12.).color(t.muted))
                .child(label(
                    if s.demo {
                        "Preview · sample conversations"
                    } else {
                        "End-to-end encrypted"
                    },
                    10.,
                    400.,
                    t.muted,
                )),
        )
}
fn clock(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .filter(|_| ms > 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}
fn message(m: &Message, t: &Theme, i: usize, max_width: f32) -> El {
    let status = if m.outgoing {
        match m.delivery {
            Delivery::Pending => "Sending…",
            Delivery::Sent => "Sent",
            Delivery::Delivered => "Delivered",
            Delivery::Read => "Read",
            Delivery::Failed => "Failed · copy to retry",
        }
    } else {
        ""
    };
    row()
        .w_full()
        .child(if m.outgoing { el().grow(1.) } else { el() })
        .child(
            col()
                .key(format!("message-{i}"))
                .max_w(max_width)
                .min_w(60.)
                .padding(12.)
                .gap(5.)
                .radius(16.)
                .fill(if m.outgoing { t.blue } else { t.incoming })
                .child(
                    label(
                        &m.body,
                        14.,
                        400.,
                        if m.outgoing { Color::WHITE } else { t.fg },
                    )
                    .wrap_anywhere()
                    .selectable(true),
                )
                .child(
                    label(
                        format!(
                            "{}  {}{}",
                            clock(m.timestamp),
                            status,
                            if m.receipts.len() > 1 {
                                format!(
                                    " · {}",
                                    m.receipts
                                        .values()
                                        .filter(|d| **d == Delivery::Read)
                                        .count()
                                )
                            } else {
                                String::new()
                            }
                        ),
                        10.,
                        400.,
                        if m.delivery == Delivery::Failed {
                            color("#ffe0c9")
                        } else if m.outgoing {
                            Color::WHITE
                        } else {
                            t.muted
                        },
                    )
                    .self_end(),
                ),
        )
        .child(if !m.outgoing { el().grow(1.) } else { el() })
}
fn conversation(s: &State, t: &Theme, k: &Kit, max_width: f32) -> El {
    let c = s
        .selected
        .as_ref()
        .and_then(|id| s.conversations.iter().find(|c| &c.id == id));
    let Some(c) = c else {
        return col()
            .grow(1.)
            .h_full()
            .center()
            .gap(18.)
            .child(
                el().size(100., 100.)
                    .radius(50.)
                    .fill(t.blue)
                    .center()
                    .child(icon(&CHAT, 54.).color(Color::WHITE)),
            )
            .child(label("Signal Rune", 26., 600., t.fg))
            .child(label(
                "Select a conversation or start a new message.",
                14.,
                400.,
                t.muted,
            ))
            .child(label(
                "Your conversations stay between you.",
                12.,
                400.,
                t.muted,
            ));
    };
    let mut messages = col().w_full().px(28.).py(20.).gap(10.);
    messages = messages.child(row().w_full().justify_center().py(8.).child(label(
        if s.demo { "Today" } else { "Messages" },
        11.,
        500.,
        t.muted,
    )));
    if c.messages.is_empty() {
        messages = messages.child(
            label(
                "Say hello. Messages are end-to-end encrypted by Signal.",
                12.,
                400.,
                t.muted,
            )
            .self_center()
            .py(24.),
        );
    }
    for (i, m) in c.messages.iter().enumerate() {
        messages = messages.child(message(m, t, i, max_width));
    }
    if let Some(typing) = typing_label(c) {
        messages = messages.child(
            row()
                .key("typing-indicator")
                .gap(8.)
                .center_y()
                .child(
                    el().size(34., 24.)
                        .radius(12.)
                        .fill(t.incoming)
                        .center()
                        .child(label("•••", 12., 600., t.muted)),
                )
                .child(label(typing, 11., 400., t.muted)),
        );
    }
    let pin_id = c.id.clone();
    col().key("conversation").grow(1.).min_w(0.).h_full()
        .child(row().h(72.).w_full().px(24.).gap(12.).center_y().border_sides([0.,0.,1.,0.],t.line)
            .child(avatar(&c.name,38.,s.conversations.iter().position(|x|x.id==c.id).unwrap_or(0)))
            .child(col().gap(4.).grow(1.).min_w(0.).child(label(&c.name,15.,600.,t.fg).truncate()).child(label(if c.blocked {"Blocked"} else if c.group{"Group conversation"}else{"Signal conversation"},11.,400.,t.muted)))
            .child(icon_button("pin-conversation", if c.pinned > 0 { "Unpin conversation" } else { "Pin conversation" }, &PIN, t, move |s| s.toggle_pin(&pin_id)))
            .child(icon_button("archive-conversation", if c.archived { "Unarchive conversation" } else { "Archive conversation" }, &ARCHIVE, t, |s| s.archive_selected())))
        .child(col().key("messages").grow(1.).min_h(0.).w_full().scroll(ScrollSpec{stick_bottom:true}).on_at_bottom(|s: &mut State, at| { s.messages_at_bottom = at; if at { s.mark_selected_read(); s.persist(); } }).child(messages))
        .child(col().w_full().px(20.).py(16.).gap(8.)
            .child(row().w_full().gap(10.).center_y()
                .child(TextInput::new(&s.composer).label("Message").placeholder(if s.demo {"Sample preview · sending is disabled"}else{"Message"}).height(44.).on_edit(|s:&mut State|&mut s.composer).on_change(|s: &mut State, tx| s.composer_changed(tx)).on_submit(|s:&mut State,tx|s.send(tx)).build(k).key("composer").grow(1.).min_w(0.))
                .child(el().key("send").role("button").label("Send message").focusable(true).size(44.,44.).radius(22.).fill(t.blue).disabled(s.demo || s.composer.text.trim().is_empty() || s.bridge.is_none()).on_press_tx(|s:&mut State,tx|s.send(tx)).center().child(icon(&SEND,22.).color(Color::WHITE))))
            .child(label(if s.demo{"Preview uses fictional messages. Connect your phone to send real messages."}else{"Enter to send · End-to-end encrypted"},10.,400.,t.muted).self_center()))
}
fn linking(s: &State, t: &Theme, k: &Kit, compact: bool) -> El {
    let mut qr = el()
        .key("qr-placeholder")
        .size(232., 232.)
        .fill(Color::WHITE)
        .radius(12.)
        .center()
        .child(icon(&CHAT, 68.).color(t.blue));
    if let Some(code) = &s.qr {
        let width = code.width();
        let unit = (232. / (width + 8) as f32).floor();
        let side = (width + 8) as f32 * unit;
        // A raster avoids antialiased seams between adjacent QR modules.
        let size = side as usize;
        let mut rgba = vec![255u8; size * size * 4];
        for y in 0..width {
            for x in 0..width {
                if code[(x, y)] == qrcode::Color::Dark {
                    for py in (y + 4) * unit as usize..(y + 5) * unit as usize {
                        for px in (x + 4) * unit as usize..(x + 5) * unit as usize {
                            let offset = (py * size + px) * 4;
                            rgba[offset..offset + 3].fill(0);
                        }
                    }
                }
            }
        }
        let pixels = rune::Pixels::new();
        pixels.set(size as u32, size as u32, rgba);
        qr = el()
            .key("link-qr")
            .label("Scan this QR code using Signal Linked Devices on your phone")
            .size(232., 232.)
            .center()
            .fill(Color::WHITE)
            .radius(12.)
            .child(el().pixels(&pixels).size(side, side));
    }
    let mut panel = col()
        .max_w(660.)
        .gap(if compact { 14. } else { 28. })
        .center_y()
        .child(
            el().size(
                if compact { 0. } else { 64. },
                if compact { 0. } else { 64. },
            )
            .radius(20.)
            .fill(t.blue)
            .center()
            .child(if compact {
                el()
            } else {
                icon(&CHAT, 36.).color(Color::WHITE)
            }),
        )
        .child(
            col()
                .gap(10.)
                .center_y()
                .child(label("Link your phone to Signal Rune", 28., 600., t.fg))
                .child(label(
                    "Your private conversations. Now on your desktop.",
                    14.,
                    400.,
                    t.muted,
                )),
        )
        .child(
            row().gap(48.).center_y().child(qr).child(
                col()
                    .w(275.)
                    .gap(22.)
                    .child(step("1", "Open Signal on your phone", t))
                    .child(step("2", "Go to Settings → Linked devices", t))
                    .child(step("3", "Tap Link new device and scan this code", t)),
            ),
        )
        .child(label(&s.status, 12., 500., t.muted))
        .child(
            Button::new(if s.linking {
                "Waiting for your phone…"
            } else {
                "Link a device"
            })
            .variant(ButtonVariant::Contrast)
            .disabled(s.linking || s.bridge.is_none())
            .on_press(|s: &mut State| s.start_link())
            .build(k)
            .key("link-device"),
        )
        .child(label(
            "Independent experimental client · Built with Rune",
            11.,
            400.,
            t.muted,
        ));
    if s.bridge.is_none() {
        panel = panel.child(
            Button::new("Retry connection")
                .on_press(|s: &mut State| s.connect())
                .build(k)
                .key("retry"),
        );
    }
    col().grow(1.).h_full().center().padding(32.).child(panel)
}
fn step(n: &str, s: &str, t: &Theme) -> El {
    row()
        .gap(12.)
        .center_y()
        .child(
            el().size(26., 26.)
                .radius(13.)
                .fill(t.hover)
                .center()
                .child(label(n, 12., 600., t.fg)),
        )
        .child(label(s, 13., 400., t.fg).max_w(235.).wrap())
}
fn settings(s: &State, t: &Theme, k: &Kit) -> El {
    col().grow(1.).h_full().min_h(0.).scroll(ScrollSpec { stick_bottom: false }).child(col().w_full().padding(32.).gap(24.)
        .child(row().gap(12.).center_y().child(icon_button("back","Back to chats",&BACK,t,|s|s.settings=false)).child(label("Settings",24.,600.,t.fg)))
        .child(label("Appearance",16.,600.,t.fg))
        .child(Button::new(if s.dark{"Switch to light theme"}else{"Switch to dark theme"}).on_press(|s:&mut State|{ s.dark = !s.dark; s.dirty = true; s.persist(); }).build(k).key("theme-toggle"))
        .child(el().h(1.).w_full().fill(t.line))
        .child(label("Connection",16.,600.,t.fg)).child(label(&s.status,13.,400.,t.muted))
        .child(label(s.account.as_deref().unwrap_or("No linked account"),13.,400.,t.fg).selectable(true))
        .child(Button::new("Retry connection").disabled(s.demo).on_press(|s:&mut State|s.connect()).build(k).key("reconnect"))
        .child(label("History",16.,600.,t.fg))
        .child(label(format!("Encrypted on this device · {}", s.history_protection),12.,400.,t.muted).wrap())
        .child(label(s.engine_configuration.as_ref().map(|c| format!("Phone privacy · Typing {} · Read receipts {}", if c.typing_indicators { "on" } else { "off" }, if c.read_receipts { "on" } else { "off" })).unwrap_or_else(|| "Phone privacy · Waiting for synchronized settings".into()),12.,400.,t.muted).wrap())
        .child(label(s.configuration_error.as_deref().unwrap_or("Typing indicators and read receipts follow the privacy settings on your phone. Read state syncs between your linked devices."),12.,400.,t.muted).max_w(570.).wrap())
        .child(label("About Signal Rune",16.,600.,t.fg))
        .child(label("0.1.0 · Experimental native client\nRune + rune-kit · Signal transport by signal-cli\nNot affiliated with Signal Messenger LLC.",13.,400.,t.muted).wrap())
        .child(label("Messages, drafts, pins and read state are stored in authenticated encrypted history on this device. Account keys and contacts are managed by the Signal engine. Calls, attachments, stories and disappearing-message support are being implemented.",12.,400.,t.muted).max_w(570.).wrap()))
}
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let t = Theme::new(s.dark);
    let k = t.kit();
    let content = if s.settings {
        settings(s, &t, &k)
    } else if s.account.is_none() {
        linking(
            s,
            &t,
            &k,
            cx.layout_size("app-root").is_some_and(|r| r[1] < 700.),
        )
    } else {
        row()
            .grow(1.)
            .h_full()
            .min_w(0.)
            .child(sidebar(s, &t, &k))
            .child(el().w(1.).h_full().fill(t.line))
            .child(conversation(
                s,
                &t,
                &k,
                cx.layout_size("conversation")
                    .map(|r| (r[0] - 56.).clamp(180., 440.))
                    .unwrap_or(320.),
            ))
    };
    let mut body = col()
        .key("app-root")
        .w_full()
        .h_full()
        .fill(t.bg)
        .child(
            row()
                .h(32.)
                .w_full()
                .window_drag()
                .fill(t.rail)
                .justify_center()
                .center_y()
                .child(label("Signal Rune", 11., 500., t.muted)),
        )
        .child(
            row()
                .grow(1.)
                .min_h(0.)
                .w_full()
                .child(rail(s, &t))
                .child(content),
        );
    if let Some(error) = &s.error {
        body = body.child(
            row()
                .key("error-banner")
                .w_full()
                .padding(14.)
                .gap(12.)
                .center_y()
                .fill(if s.dark {
                    color("#48342d")
                } else {
                    color("#fff0df")
                })
                .child(label(error, 12., 400., t.fg).wrap().grow(1.))
                .child(
                    Button::new("Dismiss")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut State| s.error = None)
                        .build(&k),
                ),
        );
    }
    if s.new_chat {
        let dialog = col()
            .key("new-chat-dialog")
            .role("dialog")
            .label("New message")
            .w(430.)
            .padding(28.)
            .gap(18.)
            .radius(16.)
            .fill(t.bg)
            .border(1., t.line)
            .child(label("New message", 22., 600., t.fg))
            .child(
                label(
                    "Enter a phone number with country code or a Signal account UUID.",
                    13.,
                    400.,
                    t.muted,
                )
                .wrap(),
            )
            .child(
                TextInput::new(&s.recipient)
                    .label("Recipient")
                    .placeholder("+1 555 010 0000 or UUID")
                    .on_edit(|s: &mut State| &mut s.recipient)
                    .on_submit(|s: &mut State, _| s.new_conversation())
                    .build(&k)
                    .key("recipient"),
            )
            .child(
                row()
                    .justify_end()
                    .gap(8.)
                    .child(
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .on_press(|s: &mut State| s.new_chat = false)
                            .build(&k),
                    )
                    .child(
                        Button::new("Start chat")
                            .disabled(s.recipient.text.trim().is_empty())
                            .on_press(|s: &mut State| s.new_conversation())
                            .build(&k)
                            .key("start-chat"),
                    ),
            );
        body = el().w_full().h_full().child(body.inert()).child(
            el().pin(Some(0.), Some(32.), Some(0.), Some(0.))
                .fill(Color::BLACK.alpha(0.55))
                .center()
                .child(dialog),
        );
    }
    body
}
fn demo(s: &mut State) {
    s.demo = true;
    s.engine_configuration = Some(crate::model::EngineConfiguration::default());
    s.history_protection = "Design fixture · no real account".into();
    s.account = Some("Preview account".into());
    s.status = "Design preview".into();
    let names = [
        "Alex Morgan",
        "Weekend plans",
        "Maya Chen",
        "Noah Williams",
        "Design studio",
        "Sofia Patel",
        "Note to Self",
    ];
    for (i, n) in names.iter().enumerate() {
        s.conversations.push(Conversation::new(
            format!("demo-{i}"),
            n.to_string(),
            i == 1 || i == 4,
        ));
    }
    let lines = [
        (
            false,
            "Hey! Did you get a chance to look at the photos from the weekend?",
        ),
        (true, "I did! That light on the lake was incredible."),
        (false, "Right? I’m so glad we took the long way back."),
        (false, "We should do it again before it gets too cold."),
        (true, "Absolutely. How does next Saturday look?"),
        (false, "Perfect. Let’s make a day of it."),
        (true, "Sounds like a plan. I’ll bring coffee this time."),
    ];
    for (i, (out, body)) in lines.iter().enumerate() {
        s.conversations[0].messages.push(Message {
            id: i as u64,
            body: body.to_string(),
            outgoing: *out,
            timestamp: 1_800_000_000_000 + i as u64 * 60_000,
            delivery: Delivery::Sent,
            ..Message::default()
        });
    }
    for (i, c) in s.conversations.iter_mut().enumerate().skip(1) {
        c.messages.push(Message {
            id: 100 + i as u64,
            body: [
                "See you all on Saturday!",
                "Thank you, that looks great",
                "Are we still on for tomorrow?",
                "Maya: Shared the latest sketches",
                "You: Talk soon!",
                "Ideas for the next adventure",
            ][i - 1]
                .into(),
            outgoing: false,
            timestamp: 1_800_000_000_000 - i as u64 * 1_800_000,
            delivery: Delivery::Sent,
            ..Message::default()
        });
    }
    s.conversations[1].unread = 3;
    s.conversations[2].unread = 1;
    s.selected = Some("demo-0".into());
}
type Scene = (&'static str, fn(&mut State));
pub const SCENES: &[Scene] = &[
    ("typing", |s| {
        demo(s);
        s.conversations[0].typing.insert(
            "demo-peer".into(),
            crate::model::TypingPresence {
                name: "Alex Morgan".into(),
                until: 100.,
            },
        );
    }),
    ("receipts", |s| {
        demo(s);
        let messages = &mut s.conversations[0].messages;
        messages[1].delivery = Delivery::Sent;
        messages[4].delivery = Delivery::Delivered;
        messages[6].delivery = Delivery::Read;
    }),
    ("pinned", |s| {
        demo(s);
        s.conversations[2].pinned = 2;
        s.conversations[4].pinned = 1;
    }),
    ("archived", |s| {
        demo(s);
        s.conversations[1].archived = true;
        s.show_archived = true;
        s.selected = None;
    }),
    ("long-message", |s| {
        demo(s);
        s.conversations[0].messages[0].body = "A".repeat(200);
    }),
    ("qr-preview", |s| {
        s.demo = true;
        s.linking = true;
        s.status = "Design fixture — not a usable device link".into();
        s.qr = qrcode::QrCode::new(format!(
            "https://example.invalid/signal-rune-design-fixture?payload={}",
            "sample-".repeat(16)
        ))
        .ok();
    }),
    ("chat-dark", demo),
    ("chat-light", |s| {
        demo(s);
        s.dark = false;
    }),
    ("link-dark", |s| {
        s.status = "Ready to link your phone".into();
    }),
    ("link-light", |s| {
        s.dark = false;
        s.status = "Ready to link your phone".into();
    }),
    ("settings", |s| {
        demo(s);
        s.settings = true;
    }),
    ("new-message", |s| {
        demo(s);
        s.new_chat = true;
    }),
    ("empty", |s| {
        s.demo = true;
        s.account = Some("Preview".into());
    }),
    ("error", |s| {
        s.error=Some("Signal engine could not start. Install signal-cli 0.14.10 and Java 25, then retry connection.".into());
    }),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normal_text_palette_meets_wcag_aa_contrast() {
        let contrast = |a: Color, b: Color| {
            let a = a.luminance();
            let b = b.luminance();
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        };
        for dark in [false, true] {
            let t = Theme::new(dark);
            for bg in [t.bg, t.sidebar, t.rail, t.incoming, t.selected] {
                assert!(contrast(t.fg, bg) >= 4.5);
                assert!(contrast(t.muted, bg) >= 4.5);
            }
            assert!(contrast(Color::WHITE, t.blue) >= 4.5);
        }
    }
}
