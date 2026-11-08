mod dialogs;
use crate::{Conversation, Delivery, Message, State};
use rune::{Color, Cx, El, Icon, ScrollSpec, TextStyle, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, PaletteSeed, TextArea, TextInput};

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
static MORE: Icon = Icon {
    name: "more",
    view: 24.,
    body: r#"<circle cx="5" cy="12" r="1.7" fill="currentColor"/><circle cx="12" cy="12" r="1.7" fill="currentColor"/><circle cx="19" cy="12" r="1.7" fill="currentColor"/>"#,
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
static ATTACH: Icon = Icon {
    name: "attach",
    view: 24.,
    body: r#"<path d="m8 13 6-6a3 3 0 0 1 4 4l-8 8a5 5 0 0 1-7-7l9-9a2 2 0 0 1 3 3l-9 9" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#,
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
        .filter(|(_, c)| {
            c.archived == s.show_archived && (c.active || c.messages.iter().any(|m| !m.hidden))
        })
        .collect();
    conversations.sort_by(|(_, a), (_, b)| {
        (b.pinned > 0)
            .cmp(&(a.pinned > 0))
            .then_with(|| a.pinned.cmp(&b.pinned))
            .then_with(|| {
                b.last_message()
                    .map(|m| m.timestamp)
                    .unwrap_or(0)
                    .cmp(&a.last_message().map(|m| m.timestamp).unwrap_or(0))
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
            c.last_message()
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
                                    c.last_message()
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
struct MessageLayout {
    width: f32,
    anchor: rune::PointerPress,
}
fn message(
    s: &State,
    c: &Conversation,
    m: &Message,
    t: &Theme,
    k: &Kit,
    layout: MessageLayout,
    layer: &mut Vec<El>,
) -> El {
    let ink = if m.outgoing { Color::WHITE } else { t.fg };
    let muted = if m.outgoing { Color::WHITE } else { t.muted };
    let status = if m.outgoing {
        match m.delivery {
            Delivery::Pending => "Sending…",
            Delivery::Sent => "Sent",
            Delivery::Delivered => "Delivered",
            Delivery::Read => "Read",
            Delivery::Failed => "Failed",
        }
    } else {
        ""
    };
    let mut bubble = col()
        .key(format!("message-{}", m.id))
        .max_w(layout.width)
        .min_w(60.)
        .padding(12.)
        .gap(5.)
        .radius(16.)
        .fill(if m.outgoing { t.blue } else { t.incoming });
    if c.group && !m.outgoing && !m.sender.is_empty() {
        bubble = bubble.child(label(s.display_name(&m.sender), 11., 600., ink));
    }
    if let Some(quote) = &m.quote {
        let target = c
            .messages
            .iter()
            .find(|target| {
                target.matches_timestamp(quote.timestamp)
                    && (target.sender == quote.author
                        || (target.outgoing && s.account.as_deref() == Some(&quote.author)))
            })
            .map(|m| m.id);
        bubble = bubble.child(
            col()
                .key(format!("quote-{}", m.id))
                .w_full()
                .padding(8.)
                .gap(4.)
                .border_sides([0., 0., 0., 2.], ink)
                .fill(ink.alpha(0.08))
                .radius(4.)
                .on_press_tx(move |_: &mut State, tx| {
                    if let Some(target) = target {
                        tx.scroll_into_view(format!("message-{target}"));
                    }
                })
                .child(label(s.display_name(&quote.author), 11., 600., ink))
                .child(
                    label(&quote.body, 12., 400., ink)
                        .wrap_anywhere()
                        .max_h(48.),
                ),
        );
    }
    for a in &m.attachments {
        let attachment = a.clone();
        let mut card = col()
            .key(format!("attachment-{}-{}", m.id, a.id))
            .gap(6.)
            .padding(8.)
            .radius(8.)
            .fill(ink.alpha(0.07))
            .role("button")
            .label(format!("Save {}", a.name))
            .focusable(true)
            .on_press_tx(move |s: &mut State, tx| s.save_attachment(&attachment, tx));
        if let Some(image) = a.media_id.as_ref().and_then(|id| s.media_images.get(id)) {
            let width = (layout.width - 48.).clamp(120., 300.);
            let height = image
                .size()
                .map_or(160., |[w, h]| width * h as f32 / w.max(1) as f32)
                .clamp(64., 220.);
            card = card.child(
                el().w(width)
                    .h(height)
                    .radius(6.)
                    .pixels_fit(image.pixels(), rune::SurfaceFit::Fit),
            );
        }
        card = card
            .child(label(&a.name, 13., 500., ink).wrap_anywhere())
            .child(label(
                format!(
                    "{} · {}",
                    crate::media::size(a.size),
                    if a.media_id.is_some() {
                        "Save"
                    } else if s.media_failures.contains_key(&a.id) {
                        "Retry download"
                    } else {
                        "Downloading…"
                    }
                ),
                10.,
                400.,
                muted,
            ));
        bubble = bubble.child(card);
    }
    if !m.body.is_empty() || m.deleted {
        bubble = bubble.child(
            label(
                if m.deleted {
                    "This message was deleted"
                } else {
                    &m.body
                },
                14.,
                400.,
                ink,
            )
            .font(if m.deleted {
                TextStyle::new(14., 400.).italic()
            } else {
                TextStyle::new(14., 400.)
            })
            .wrap_anywhere()
            .selectable(true),
        );
    }
    if !m.reactions.is_empty() {
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for emoji in m.reactions.values() {
            *counts.entry(emoji.clone()).or_default() += 1;
        }
        let mut reactions = row().gap(4.).flex_wrap();
        for (emoji, count) in counts {
            let peer = c.id.clone();
            let mid = m.id;
            let sent = emoji.clone();
            let own = s.account.as_ref().and_then(|a| m.reactions.get(a)) == Some(&emoji);
            reactions = reactions.child(
                el().key(format!("reaction-{mid}-{emoji}"))
                    .role("button")
                    .label(format!("{emoji}, {count} reactions"))
                    .focusable(true)
                    .px(8.)
                    .py(4.)
                    .radius(10.)
                    .fill(ink.alpha(if own { 0.22 } else { 0.1 }))
                    .on_press(move |s: &mut State| {
                        s.queue_action(
                            &peer,
                            mid,
                            crate::model::ActionKind::Reaction {
                                emoji: sent.clone(),
                                remove: own,
                            },
                        );
                    })
                    .child(label(format!("{emoji} {count}"), 12., 500., ink)),
            );
        }
        bubble = bubble.child(reactions);
    }
    let peer = c.id.clone();
    let mid = m.id;
    let mut meta = row().gap(6.).self_end().center_y();
    if m.expires_in > 0 {
        meta = meta.child(
            label(
                format!("◷ {}", crate::expiration::duration(m.expires_in)),
                10.,
                400.,
                muted,
            )
            .label("Disappearing message"),
        );
    }
    if m.edited {
        meta = meta.child(
            label("Edited", 10., 500., muted)
                .key(format!("edited-{mid}"))
                .role("button")
                .label("View edit history")
                .focusable(true)
                .on_press(move |s: &mut State| s.message_details = Some((peer.clone(), mid))),
        );
    }
    let read_count = m
        .receipts
        .values()
        .filter(|d| **d == Delivery::Read)
        .count();
    meta = meta.child(label(
        format!(
            "{}  {}{}",
            clock(m.timestamp),
            status,
            if c.group && read_count > 0 {
                format!(" · {read_count}")
            } else {
                String::new()
            }
        ),
        10.,
        400.,
        muted,
    ));
    bubble = bubble.child(meta);
    let peer = c.id.clone();
    let mid = m.id;
    bubble = bubble.on_secondary_press(move |s: &mut State, point, _| {
        s.message_context = Some(crate::actions::MessageContext {
            conversation: peer.clone(),
            message: mid,
            point,
        })
    });
    let point = s
        .message_context
        .as_ref()
        .filter(|ctx| ctx.conversation == c.id && ctx.message == m.id)
        .map(|ctx| ctx.point);
    let mut menu = rune_kit::context_menu(format!("message-menu-{}", m.id), bubble, point)
        .on_close(|s: &mut State| s.message_context = None);
    let peer = c.id.clone();
    let mid = m.id;
    menu = menu.item(
        rune_kit::MenuItem::new("Reply")
            .disabled(m.deleted)
            .on_select(move |s: &mut State| s.start_reply(&peer, mid)),
    );
    let peer = c.id.clone();
    menu = menu.item(
        rune_kit::MenuItem::new("React")
            .disabled(m.deleted)
            .on_select(move |s: &mut State| {
                s.message_context = None;
                s.reaction_target = Some((peer.clone(), mid));
            }),
    );
    let copy = m.body.clone();
    menu = menu.item(
        rune_kit::MenuItem::new("Copy text")
            .disabled(m.deleted)
            .on_select_tx(move |s: &mut State, tx| {
                tx.copy(copy.clone());
                s.message_context = None;
            }),
    );
    let note = s.account.as_ref().is_some_and(|a| c.matches(a));
    if m.outgoing {
        let peer = c.id.clone();
        menu = menu.item(
            rune_kit::MenuItem::new("Edit message")
                .disabled(!m.can_edit(note))
                .on_select(move |s: &mut State| s.start_edit(&peer, mid)),
        );
        if m.delivery == Delivery::Failed
            || s.sync_ledger
                .actions
                .iter()
                .any(|a| a.conversation == c.id && a.message == mid && a.failed)
        {
            let peer = c.id.clone();
            menu = menu.item(
                rune_kit::MenuItem::new("Retry")
                    .on_select(move |s: &mut State| s.retry_message(&peer, mid)),
            );
        }
    }
    let peer = c.id.clone();
    menu = menu
        .item(
            rune_kit::MenuItem::new("Message details").on_select(move |s: &mut State| {
                s.message_context = None;
                s.message_details = Some((peer.clone(), mid));
            }),
        )
        .separator();
    let peer = c.id.clone();
    menu = menu.item(
        rune_kit::MenuItem::new("Delete for me")
            .danger(true)
            .on_select(move |s: &mut State| {
                s.message_context = None;
                s.delete_confirmation = Some((peer.clone(), mid, false));
            }),
    );
    if m.can_delete_for_everyone() {
        let peer = c.id.clone();
        menu = menu.item(
            rune_kit::MenuItem::new("Delete for everyone")
                .danger(true)
                .on_select(move |s: &mut State| {
                    s.message_context = None;
                    s.delete_confirmation = Some((peer.clone(), mid, true));
                }),
        );
    }
    let bubble = menu.build(k, layer);
    let peer = c.id.clone();
    let anchor = layout.anchor;
    let more = el()
        .key(format!("message-more-{mid}"))
        .size(28., 28.)
        .radius(14.)
        .role("button")
        .label("Message options")
        .help("Message options")
        .focusable(true)
        .group_opacity(0., 1.)
        .hover(t.hover)
        .center()
        .self_center()
        .on_press(move |s: &mut State| {
            s.message_context = Some(crate::actions::MessageContext {
                conversation: peer.clone(),
                message: mid,
                point: anchor,
            })
        })
        .child(icon(&MORE, 18.).color(t.muted));
    let line = row().w_full().gap(6.).hover_group_focus();
    if m.outgoing {
        line.child(el().grow(1.)).child(more).child(bubble)
    } else {
        line.child(bubble).child(more).child(el().grow(1.))
    }
}
fn conversation(s: &State, t: &Theme, k: &Kit, cx: &mut Cx<State>, layer: &mut Vec<El>) -> El {
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
    let max_width = cx
        .layout_size("conversation")
        .map(|r| (r[0] - 96.).clamp(160., 440.))
        .unwrap_or(280.);
    for m in c.messages.iter().filter(|m| !m.hidden) {
        let rect = cx
            .window_layout(&format!("message-{}", m.id))
            .unwrap_or([400., 100., max_width, 40.]);
        let anchor = rune::PointerPress {
            x: if m.outgoing {
                rect[0] - 30.
            } else {
                rect[0] + rect[2] + 4.
            },
            y: rect[1],
        };
        messages = messages.child(message(
            s,
            c,
            m,
            t,
            k,
            MessageLayout {
                width: max_width,
                anchor,
            },
            layer,
        ));
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
    let drop_peer = c.id.clone();
    let details_peer = c.id.clone();
    col().key("conversation").grow(1.).min_w(0.).h_full().on_drop_files(move |s:&mut State,paths,_| s.import_files(&drop_peer,paths))
        .child(row().h(72.).w_full().px(24.).gap(12.).center_y().border_sides([0.,0.,1.,0.],t.line)
            .child(avatar(&c.name,38.,s.conversations.iter().position(|x|x.id==c.id).unwrap_or(0)))
            .child(col().key("conversation-details").role("button").label("Conversation details").focusable(true).on_press(move |s:&mut State|s.open_conversation_info(&details_peer)).gap(4.).grow(1.).min_w(0.).child(label(&c.name,15.,600.,t.fg).truncate()).child(label(if c.blocked {"Blocked"} else if c.expiration > 0 {"Disappearing messages enabled"} else if c.group{"Group conversation"}else{"Signal conversation"},11.,400.,t.muted)))
            .child(icon_button("pin-conversation", if c.pinned > 0 { "Unpin conversation" } else { "Pin conversation" }, &PIN, t, move |s| s.toggle_pin(&pin_id)))
            .child(icon_button("archive-conversation", if c.archived { "Unarchive conversation" } else { "Archive conversation" }, &ARCHIVE, t, |s| s.archive_selected())))
        .child(col().key("messages").grow(1.).min_h(0.).w_full().scroll(ScrollSpec{stick_bottom:true}).on_at_bottom(|s: &mut State, at| { s.messages_at_bottom = at; if at { s.mark_selected_read(); s.persist(); } }).child(messages))
        .child(col().w_full().px(20.).py(16.).gap(8.)
            .child(compose_context(s,t,k))
            .child(draft_media(c,t,k))
            .child(row().w_full().gap(10.).center_y()
                .child(el().key("attach-file").role("button").label("Attach a file").focusable(true).size(32.,44.).disabled(s.demo || !s.history_ready || s.editing.is_some()).on_press_tx(|s:&mut State,tx|s.choose_attachment(tx)).center().child(icon(&ATTACH,21.).color(t.muted)))
                .child(TextArea::new(&s.composer).label("Message").placeholder(if s.demo {"Sample preview · sending is disabled"}else{"Message"}).auto_grow(1,6).on_edit(|s:&mut State|&mut s.composer).on_change(|s: &mut State, tx| s.composer_changed(tx)).on_key(|s: &mut State, ev, tx| s.composer_key(ev,tx)).build(k).min_h(44.).justify_center().key("composer").grow(1.).min_w(0.))
                .child(el().key("send").role("button").label(if s.editing.is_some() {"Save changes"} else {"Send message"}).focusable(true).size(44.,44.).radius(22.).fill(t.blue).disabled(s.demo || (s.composer.text.trim().is_empty() && c.draft_attachments.is_empty()) || !s.history_ready).on_press_tx(|s:&mut State,tx|s.send(tx)).center().child(icon(&SEND,22.).color(Color::WHITE))))
            .child(label(if s.demo{"Preview uses fictional messages. Connect your phone to send real messages."}else{"Enter to send · Shift+Enter for a new line"},10.,400.,t.muted).self_center()))
}
fn draft_media(c: &Conversation, t: &Theme, k: &Kit) -> El {
    if c.draft_attachments.is_empty() {
        return el();
    }
    let mut files = col()
        .key("draft-attachments")
        .gap(4.)
        .w_full()
        .max_h(96.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    for (index, a) in c.draft_attachments.iter().enumerate() {
        files = files.child(
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .padding(6.)
                .fill(t.hover)
                .radius(6.)
                .child(icon(&ATTACH, 18.).color(t.muted))
                .child(
                    label(
                        format!("{} · {}", a.name, crate::media::size(a.size)),
                        12.,
                        400.,
                        t.fg,
                    )
                    .truncate()
                    .grow(1.)
                    .min_w(0.),
                )
                .child(
                    Button::new("×")
                        .on_press(move |s: &mut State| s.remove_draft_attachment(index))
                        .build(k)
                        .key(format!("remove-attachment-{index}")),
                ),
        );
    }
    files
}
fn qr_element(code: &qrcode::QrCode, points: f32, key: &str, name: &str) -> El {
    let width = code.width();
    let unit = (points / (width + 8) as f32).floor().max(1.) as usize;
    let side = (width + 8) * unit;
    let mut rgba = vec![255u8; side * side * 4];
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] == qrcode::Color::Dark {
                for py in (y + 4) * unit..(y + 5) * unit {
                    for px in (x + 4) * unit..(x + 5) * unit {
                        rgba[(py * side + px) * 4..(py * side + px) * 4 + 3].fill(0);
                    }
                }
            }
        }
    }
    let pixels = rune::Pixels::new();
    pixels.set(side as u32, side as u32, rgba);
    el().key(key)
        .label(name)
        .size(points, points)
        .center()
        .fill(Color::WHITE)
        .radius(12.)
        .child(el().pixels(&pixels).size(side as f32, side as f32))
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
        qr = qr_element(
            code,
            232.,
            "link-qr",
            "Scan this QR code using Signal Linked Devices on your phone",
        );
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
        .child(label(s.pin_sync_status(),12.,400.,t.muted).wrap())
        .child(label("About Signal Rune",16.,600.,t.fg))
        .child(label("0.1.0 · Experimental native client\nRune + rune-kit · Signal transport by signal-cli\nNot affiliated with Signal Messenger LLC.",13.,400.,t.muted).wrap())
        .child(label("Messages, drafts, pins and read state are stored in authenticated encrypted history on this device. Account keys and contacts are managed by the Signal engine. Phone history transfer, calls, stories and full media support remain in development.",12.,400.,t.muted).max_w(570.).wrap()))
}
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let t = Theme::new(s.dark);
    let k = t.kit();
    let mut layer = Vec::new();
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
            .child(conversation(s, &t, &k, cx, &mut layer))
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
                    "Choose a contact, enter an international number or UUID, or paste a Signal group link.",
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
            .child(contact_picker(s, &t))
            .child(
                row()
                    .justify_end()
                    .gap(8.)
                            .child(Button::new("New group").variant(ButtonVariant::Ghost).on_press(|s:&mut State|s.start_new_group()).build(&k).key("new-group"))
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
    dialogs::conversation_dialogs(s, &t, &k, &mut layer);
    message_dialogs(s, &t, &k, &mut layer);
    rune_kit::root(body, layer)
}
fn contact_picker(s: &State, t: &Theme) -> El {
    let query = s.recipient.text.trim().to_lowercase();
    let mut contacts: Vec<_> = s
        .conversations
        .iter()
        .filter(|c| {
            !c.blocked
                && (query.is_empty()
                    || c.name.to_lowercase().contains(&query)
                    || c.aliases.iter().any(|a| a.to_lowercase().contains(&query)))
        })
        .collect();
    contacts.sort_by_key(|c| c.name.to_lowercase());
    let mut choices = col().w_full().gap(2.);
    for (i, c) in contacts.into_iter().take(50).enumerate() {
        let id = c.id.clone();
        choices = choices.child(
            row()
                .key(format!("contact-option-{i}"))
                .role("button")
                .label(&c.name)
                .focusable(true)
                .w_full()
                .h(52.)
                .px(8.)
                .gap(10.)
                .center_y()
                .radius(8.)
                .hover(t.hover)
                .on_press(move |s: &mut State| {
                    s.choose(id.clone());
                    s.new_chat = false;
                    s.recipient.clear();
                    s.dirty = true;
                    s.persist();
                })
                .child(avatar(&c.name, 34., i))
                .child(
                    col()
                        .grow(1.)
                        .min_w(0.)
                        .gap(3.)
                        .child(label(&c.name, 13., 600., t.fg).truncate())
                        .child(
                            label(
                                if c.group {
                                    "Group"
                                } else {
                                    c.aliases
                                        .iter()
                                        .find(|a| a.starts_with('+'))
                                        .map(String::as_str)
                                        .unwrap_or("Signal contact")
                                },
                                10.,
                                400.,
                                t.muted,
                            )
                            .truncate(),
                        ),
                ),
        );
    }
    col()
        .key("contact-picker")
        .w_full()
        .max_h(180.)
        .min_h(0.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(choices)
}
fn compose_context(s: &State, t: &Theme, k: &Kit) -> El {
    if s.editing.is_none() && s.reply_to.is_none() {
        return el();
    }
    let title = if s.editing.is_some() {
        "Editing message".to_owned()
    } else {
        format!(
            "Replying to {}",
            s.reply_to
                .as_ref()
                .map(|q| s.display_name(&q.author))
                .unwrap_or_default()
        )
    };
    let preview = s
        .reply_to
        .as_ref()
        .map(|q| q.body.clone())
        .unwrap_or_default();
    row()
        .key("compose-context")
        .w_full()
        .gap(12.)
        .center_y()
        .padding(10.)
        .radius(8.)
        .fill(t.hover)
        .border_sides([0., 0., 0., 3.], t.blue)
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .gap(4.)
                .child(label(title, 12., 600., t.fg))
                .child(label(preview, 11., 400., t.muted).truncate()),
        )
        .child(
            Button::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut State| {
                    if s.editing.is_some() {
                        s.cancel_edit();
                    } else {
                        s.reply_to = None;
                        s.dirty = true;
                        s.persist();
                    }
                })
                .build(k)
                .key("cancel-compose-context"),
        )
}
fn message_dialogs(s: &State, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    if let Some((peer, id)) = &s.reaction_target {
        let mut choices = row().gap(8.).flex_wrap();
        for emoji in ["👍", "❤️", "😂", "😮", "😢", "🙏"] {
            let peer = peer.clone();
            let id = *id;
            choices = choices.child(
                Button::new(emoji)
                    .on_press(move |s: &mut State| {
                        s.queue_action(
                            &peer,
                            id,
                            crate::model::ActionKind::Reaction {
                                emoji: emoji.into(),
                                remove: false,
                            },
                        );
                    })
                    .build(k)
                    .key(format!("react-{emoji}")),
            );
        }
        Dialog::new("reaction", "React to message")
            .fit()
            .open(true)
            .width(420.)
            .on_close(|s: &mut State| s.reaction_target = None)
            .child(choices.padding(16.))
            .build(k, layer);
    }
    if let Some((peer, id, everyone)) = &s.delete_confirmation {
        let peer = peer.clone();
        let id = *id;
        let everyone = *everyone;
        let body = col()
            .gap(18.)
            .child(
                label(
                    if everyone {
                        "Delete this message for everyone in this conversation?"
                    } else {
                        "Delete this message from this device?"
                    },
                    13.,
                    400.,
                    t.fg,
                )
                .wrap(),
            )
            .child(
                row()
                    .justify_end()
                    .gap(8.)
                    .child(
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .on_press(|s: &mut State| s.delete_confirmation = None)
                            .build(k),
                    )
                    .child(
                        Button::new(if everyone {
                            "Delete for everyone"
                        } else {
                            "Delete for me"
                        })
                        .variant(ButtonVariant::Danger)
                        .on_press(move |s: &mut State| {
                            if everyone {
                                s.queue_action(&peer, id, crate::model::ActionKind::Delete);
                                s.delete_confirmation = None;
                            } else {
                                s.delete_for_me(&peer, id);
                            }
                        })
                        .build(k)
                        .key("confirm-message-delete"),
                    ),
            );
        Dialog::new("delete-message", "Delete message")
            .fit()
            .open(true)
            .width(440.)
            .on_close(|s: &mut State| s.delete_confirmation = None)
            .child(body)
            .build(k, layer);
    }
    if let Some((peer, id)) = &s.message_details
        && let Some(m) = s.find_message(peer, *id)
    {
        let time = chrono::DateTime::from_timestamp_millis(m.latest_timestamp() as i64)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%b %d, %Y · %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default();
        let mut details = col()
            .gap(14.)
            .child(label(
                if m.outgoing {
                    "You".to_owned()
                } else {
                    s.display_name(&m.sender)
                },
                14.,
                600.,
                t.fg,
            ))
            .child(label(time, 12., 400., t.muted))
            .child(
                label(
                    if m.deleted {
                        "This message was deleted"
                    } else {
                        &m.body
                    },
                    14.,
                    400.,
                    t.fg,
                )
                .wrap_anywhere()
                .selectable(true),
            );
        if m.outgoing {
            details = details.child(label(format!("Status: {:?}", m.delivery), 12., 500., t.fg));
            for (recipient, delivery) in &m.receipts {
                details = details.child(label(
                    format!("{} · {:?}", s.display_name(recipient), delivery),
                    12.,
                    400.,
                    t.muted,
                ));
            }
        }
        if !m.edit_history.is_empty() {
            details = details.child(label("Edit history", 14., 600., t.fg));
            for version in m.edit_history.iter().rev() {
                details = details.child(
                    col()
                        .gap(4.)
                        .padding(10.)
                        .radius(6.)
                        .fill(t.hover)
                        .child(label(clock(version.timestamp), 10., 400., t.muted))
                        .child(label(&version.body, 13., 400., t.fg).wrap_anywhere()),
                );
            }
        }
        if s.sync_ledger
            .actions
            .iter()
            .any(|a| a.conversation == *peer && a.message == *id && a.failed)
            || m.delivery == Delivery::Failed
        {
            let peer = peer.clone();
            let id = *id;
            details = details.child(
                Button::new("Retry")
                    .on_press(move |s: &mut State| s.retry_message(&peer, id))
                    .build(k)
                    .key("retry-message-action"),
            );
        }
        Dialog::new("message-details", "Message details")
            .fit()
            .open(true)
            .width(480.)
            .on_close(|s: &mut State| s.message_details = None)
            .child(
                col()
                    .max_h(400.)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(details),
            )
            .build(k, layer);
    }
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
    s.conversations[6].aliases.push("Preview account".into());
    s.conversations[1].unread = 3;
    s.conversations[2].unread = 1;
    s.selected = Some("demo-0".into());
}
fn demo_group(s: &mut State) {
    demo(s);
    s.choose("demo-1".into());
    let c = &mut s.conversations[1];
    c.group = true;
    c.group_member = true;
    c.group_description = "A place to make plans for the weekend.".into();
    c.members = vec!["demo-0".into(), "demo-2".into(), "Preview account".into()];
    c.group_admins = vec!["Preview account".into()];
    c.group_link = Some("https://signal.group/#design-fixture".into());
    s.open_conversation_info("demo-1");
}
type Scene = (&'static str, fn(&mut State));
pub const SCENES: &[Scene] = &[
    ("conversation-details", |s| {
        demo(s);
        s.open_conversation_info("demo-0");
    }),
    ("group-details", demo_group),
    ("group-member", |s| {
        demo_group(s);
        s.group_member_panel = Some(("demo-1".into(), "demo-0".into()));
    }),
    ("group-confirm", |s| {
        demo_group(s);
        s.group_confirmation = Some((
            "demo-1".into(),
            crate::model::GroupOperation::Remove("demo-0".into()),
        ));
    }),
    ("new-group", |s| {
        demo(s);
        s.start_new_group();
        s.group_name_input.replace_text("Weekend plans");
        s.group_participants = vec!["demo-0".into(), "demo-2".into()];
    }),
    ("safety-number", |s| {
        demo(s);
        s.identity_panel = Some(crate::model::IdentityPanel {
            peer: "demo-0".into(),
            identity: Some(crate::model::IdentityInfo {
                safety_number: "1234567890".repeat(6),
                scannable: Some(b"signal-rune-safety-fixture".to_vec()),
                verified: false,
                trusted: true,
            }),
            loading: false,
            saving: false,
            error: None,
        });
    }),
    ("safety-loading", |s| {
        demo(s);
        s.identity_panel = Some(crate::model::IdentityPanel {
            peer: "demo-0".into(),
            identity: None,
            loading: true,
            saving: false,
            error: None,
        });
    }),
    ("attachments", |s| {
        demo(s);
        let cache = rune::ImageCache::new(std::sync::Arc::new(|| {}));
        let image = cache.load(include_bytes!("../assets/fixtures/landscape.png").as_slice());
        image.wait().expect("fixture PNG");
        s.media_images.insert("fixture-photo".into(), image);
        s.media_cache = Some(cache);
        s.conversations[0].messages.last_mut().unwrap().attachments =
            vec![crate::model::Attachment {
                id: "sample.png".into(),
                name: "Weekend.png".into(),
                content_type: "image/png".into(),
                size: 42133,
                media_id: Some("fixture-photo".into()),
                ..Default::default()
            }];
    }),
    ("attachment-draft", |s| {
        demo(s);
        s.conversations[0].draft_attachments = vec![crate::model::Attachment {
            name: "Trip itinerary.pdf".into(),
            content_type: "application/pdf".into(),
            size: 184289,
            media_id: Some("fixture-document".into()),
            ..Default::default()
        }];
        s.composer
            .replace_text("Here are the details for the trip.");
    }),
    ("multiline", |s| {
        demo(s);
        s.composer
            .replace_text("Saturday plans:\nCoffee at 10\nWalk by the lake\nLunch afterward");
    }),
    ("disappearing", |s| {
        demo(s);
        s.conversations[0].expiration = 86400;
        s.conversations[0].messages.last_mut().unwrap().expires_in = 86400;
    }),
    ("reply", |s| {
        demo(s);
        s.reply_to = Some(crate::model::Quote {
            timestamp: s.conversations[0].messages[0].timestamp,
            author: "demo-0".into(),
            body: s.conversations[0].messages[0].body.clone(),
        });
        s.composer.replace_text("Let's do it!");
    }),
    ("quoted", |s| {
        demo(s);
        s.conversations[0].messages[4].quote = Some(crate::model::Quote {
            timestamp: s.conversations[0].messages[0].timestamp,
            author: "demo-0".into(),
            body: s.conversations[0].messages[0].body.clone(),
        });
    }),
    ("reactions", |s| {
        demo(s);
        s.conversations[0].messages[0]
            .reactions
            .insert("Preview account".into(), "👍".into());
        s.conversations[0].messages[0]
            .reactions
            .insert("demo-0".into(), "❤️".into());
    }),
    ("editing", |s| {
        demo(s);
        s.editing = Some(6);
        s.composer
            .replace_text("Sounds like a plan. I'll bring tea this time.");
    }),
    ("message-menu", |s| {
        demo(s);
        s.message_context = Some(crate::actions::MessageContext {
            conversation: "demo-0".into(),
            message: 0,
            point: rune::PointerPress { x: 520., y: 170. },
        });
    }),
    ("message-details", |s| {
        demo(s);
        s.conversations[0].messages[4].edited = true;
        s.conversations[0].messages[4].edited_at =
            Some(s.conversations[0].messages[4].timestamp + 1000);
        let original_timestamp = s.conversations[0].messages[4].timestamp - 1000;
        s.conversations[0].messages[4]
            .edit_history
            .push(crate::model::EditVersion {
                timestamp: original_timestamp,
                body: "How does Friday look?".into(),
                ..crate::model::EditVersion::default()
            });
        s.message_details = Some(("demo-0".into(), 4));
    }),
    ("reaction-picker", |s| {
        demo(s);
        s.reaction_target = Some(("demo-0".into(), 0));
    }),
    ("delete-confirm", |s| {
        demo(s);
        s.delete_confirmation = Some(("demo-0".into(), 6, true));
    }),
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
