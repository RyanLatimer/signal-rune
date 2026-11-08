//! Conversation, identity and group dialogs built with native Rune components.
use super::*;

pub(super) fn conversation_dialogs(s: &State, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    if let Some(c) = s
        .conversation_info
        .as_ref()
        .and_then(|id| s.conversations.iter().find(|c| &c.id == id))
    {
        let peer = c.id.clone();
        let busy = s
            .pending
            .values()
            .any(|p| matches!(p,crate::Pending::ConversationUpdate(id,_) if id==&peer));
        let admin = s
            .account
            .as_ref()
            .is_some_and(|a| c.group_admins.contains(a));
        let can_edit =
            !c.group || (c.group_member && (c.permission_edit != "ONLY_ADMINS" || admin));
        let mut content = col().w_full().padding(16.).gap(16.);
        let rename_peer = peer.clone();
        content = content.child(
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .child(
                    TextInput::new(&s.conversation_name)
                        .label("Conversation name")
                        .on_edit(|s: &mut State| &mut s.conversation_name)
                        .build(k)
                        .grow(1.)
                        .min_w(0.)
                        .key("conversation-name"),
                )
                .child(
                    Button::new("Save")
                        .disabled(
                            busy || !can_edit
                                || s.conversation_name.text.trim().is_empty()
                                || s.conversation_name.text == c.name,
                        )
                        .on_press(move |s: &mut State| {
                            s.change_conversation(
                                &rename_peer,
                                crate::model::ConversationChange::Rename(
                                    s.conversation_name.text.trim().into(),
                                ),
                            )
                        })
                        .build(k)
                        .key("save-conversation-name"),
                ),
        );
        if !c.group_description.is_empty() {
            content = content.child(label(&c.group_description, 13., 400., t.fg).wrap_anywhere());
        }
        content = content.child(label(
            format!(
                "Disappearing messages · {}",
                crate::expiration::duration(c.expiration)
            ),
            13.,
            600.,
            t.fg,
        ));
        let mut timers = row().gap(6.).flex_wrap();
        for (seconds, title) in [
            (0, "Off"),
            (30, "30 sec"),
            (300, "5 min"),
            (3600, "1 hour"),
            (28800, "8 hours"),
            (86400, "1 day"),
            (604800, "1 week"),
            (2419200, "4 weeks"),
        ] {
            let peer = peer.clone();
            timers = timers.child(
                Button::new(title)
                    .variant(if c.expiration == seconds {
                        ButtonVariant::Submit
                    } else {
                        ButtonVariant::Outline
                    })
                    .disabled(busy || !can_edit || c.expiration == seconds)
                    .on_press(move |s: &mut State| {
                        s.change_conversation(
                            &peer,
                            crate::model::ConversationChange::Expiration(seconds),
                        )
                    })
                    .build(k)
                    .key(format!("timer-{seconds}")),
            );
        }
        content = content.child(timers).child(
            label(
                "The timer starts when a message is read or sent. Changes apply to new messages.",
                11.,
                400.,
                t.muted,
            )
            .wrap(),
        );
        let muted = c.muted_until.is_some_and(|at| at > crate::model::now_ms());
        let mute_peer = peer.clone();
        content = content.child(
            Button::new(if muted {
                "Unmute notifications"
            } else {
                "Mute for 1 hour"
            })
            .on_press(move |s: &mut State| {
                s.mute_conversation(&mute_peer, if muted { None } else { Some(3600) })
            })
            .build(k)
            .key("mute-conversation"),
        );
        if c.group {
            content = content.child(group_controls(s, c, t, k));
            content = content.child(label(
                format!("{} members", c.members.len()),
                13.,
                600.,
                t.fg,
            ));
            for member in &c.members {
                let id = member.clone();
                content = content.child(
                    row()
                        .w_full()
                        .gap(10.)
                        .center_y()
                        .child(avatar(&s.display_name(member), 30., 0))
                        .child(
                            label(s.display_name(member), 12., 500., t.fg)
                                .truncate()
                                .grow(1.)
                                .min_w(0.),
                        )
                        .child(if c.group_admins.contains(member) {
                            label("Admin", 10., 500., t.muted)
                        } else {
                            el()
                        })
                        .role("button")
                        .label("Manage group member")
                        .on_press(move |s: &mut State| {
                            s.group_member_panel =
                                Some((s.conversation_info.clone().unwrap_or_default(), id.clone()))
                        }),
                );
            }
            for (title, people) in [
                ("Pending invitations", &c.group_pending),
                ("Join requests", &c.group_requesting),
                ("Banned members", &c.group_banned),
            ] {
                if !people.is_empty() {
                    content = content.child(label(
                        format!("{title} · {}", people.len()),
                        12.,
                        600.,
                        t.fg,
                    ));
                    for person in people {
                        let group = c.id.clone();
                        let person_id = person.clone();
                        content = content.child(
                            label(s.display_name(person), 12., 400., t.muted)
                                .truncate()
                                .role("button")
                                .label("Manage invitation or join request")
                                .on_press(move |s: &mut State| {
                                    s.group_member_panel = Some((group.clone(), person_id.clone()))
                                }),
                        );
                    }
                }
            }
            if let Some(link) = &c.group_link {
                let link = link.clone();
                content = content.child(
                    Button::new("Copy group link")
                        .on_press_tx(move |_: &mut State, tx| tx.copy(link.clone()))
                        .build(k)
                        .key("copy-group-link"),
                );
            }
        } else if s.account.as_ref() != Some(&peer) {
            let id = peer.clone();
            content = content.child(
                Button::new("View safety number")
                    .on_press(move |s: &mut State| s.open_identity(&id))
                    .build(k)
                    .key("open-safety-number"),
            );
        }
        if s.account.as_ref() != Some(&peer) {
            let blocked = c.blocked;
            content = content.child(
                Button::new(if blocked { "Unblock" } else { "Block" })
                    .disabled(busy)
                    .on_press(move |s: &mut State| {
                        s.change_conversation(
                            &peer,
                            crate::model::ConversationChange::Block(!blocked),
                        )
                    })
                    .build(k)
                    .key("block-conversation"),
            );
        }
        Dialog::new(
            "conversation-info",
            if c.group {
                "Group details"
            } else {
                "Conversation details"
            },
        )
        .fit()
        .width(480.)
        .open(true)
        .on_close(|s: &mut State| s.conversation_info = None)
        .child(
            col()
                .max_h(350.)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(content),
        )
        .build(k, layer);
    }
    group_dialogs(s, t, k, layer);
    if let Some(panel) = &s.identity_panel {
        let mut content = col().padding(16.).gap(14.).w_full().center_y();
        content = content.child(label(s.display_name(&panel.peer), 16., 600., t.fg));
        if panel.loading {
            content = content.child(label("Loading safety number…", 13., 400., t.muted));
        }
        if let Some(identity) = &panel.identity {
            if let Some(bytes) = &identity.scannable
                && let Ok(code) = qrcode::QrCode::new(bytes)
            {
                content = content.child(
                    qr_element(
                        &code,
                        164.,
                        "safety-number-qr",
                        "Ask your contact to scan this safety number QR code",
                    )
                    .self_center(),
                );
            }
            let groups: Vec<_> = identity
                .safety_number
                .as_bytes()
                .chunks(5)
                .map(|c| std::str::from_utf8(c).unwrap())
                .collect();
            let numbers = groups
                .chunks(4)
                .map(|g| g.join("  "))
                .collect::<Vec<_>>()
                .join("\n");
            content=content.child(label(numbers,17.,500.,t.fg).key("safety-number-digits").self_center().selectable(true))
                .child(label(if identity.verified {"Verified"} else if !identity.trusted {"Safety number changed · compare it again"} else {"Compare these numbers with your contact in person or over a trusted channel."},12.,400.,t.muted).wrap());
            let safety = identity.safety_number.clone();
            content = content.child(
                row()
                    .w_full()
                    .gap(8.)
                    .justify_end()
                    .child(
                        Button::new("Copy")
                            .on_press_tx(move |_: &mut State, tx| tx.copy(safety.clone()))
                            .build(k)
                            .key("copy-safety-number"),
                    )
                    .child(
                        Button::new(if panel.saving {
                            "Saving…"
                        } else if identity.verified {
                            "Clear verification"
                        } else {
                            "Mark as verified"
                        })
                        .disabled(s.demo || panel.loading || panel.saving)
                        .on_press(|s: &mut State| s.verify_identity())
                        .build(k)
                        .key("verify-safety-number"),
                    ),
            );
        }
        if let Some(error) = &panel.error {
            content = content.child(label(error, 12., 400., t.fg).wrap());
        }
        let peer = panel.peer.clone();
        content = content.child(
            Button::new("Refresh")
                .disabled(s.demo || panel.loading || panel.saving)
                .on_press(move |s: &mut State| s.request_identity(&peer))
                .build(k)
                .key("refresh-safety-number"),
        );
        Dialog::new("safety-number", "Safety number")
            .fit()
            .width(460.)
            .open(true)
            .on_close(|s: &mut State| s.identity_panel = None)
            .child(
                col()
                    .max_h(430.)
                    .w_full()
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(content),
            )
            .build(k, layer);
    }
}
fn group_op_button(
    title: &str,
    peer: &str,
    op: crate::model::GroupOperation,
    busy: bool,
    confirm: bool,
    k: &Kit,
) -> El {
    let peer = peer.to_owned();
    Button::new(title)
        .disabled(busy)
        .on_press(move |s: &mut State| {
            if confirm {
                s.group_confirmation = Some((peer.clone(), op.clone()));
            } else {
                s.group_operation(&peer, op.clone());
            }
        })
        .build(k)
}
fn group_controls(s: &State, c: &Conversation, t: &Theme, k: &Kit) -> El {
    use crate::model::GroupOperation as Op;
    let admin = s
        .account
        .as_ref()
        .is_some_and(|a| c.group_admins.contains(a));
    let busy = s.demo
        || s.pending
            .values()
            .any(|p| matches!(p,crate::Pending::GroupUpdate(peer) if peer==&c.id));
    let editable =
        c.group_member && !c.group_terminated && (admin || c.permission_edit == "EVERY_MEMBER");
    let mut controls = col().w_full().gap(12.);
    if editable {
        let peer = c.id.clone();
        controls = controls
            .child(
                TextArea::new(&s.group_description_input)
                    .label("Group description")
                    .placeholder("Group description")
                    .auto_grow(2, 4)
                    .on_edit(|s: &mut State| &mut s.group_description_input)
                    .build(k)
                    .key("group-description"),
            )
            .child(
                Button::new("Save description")
                    .disabled(busy || s.group_description_input.text == c.group_description)
                    .on_press(move |s: &mut State| {
                        s.group_operation(
                            &peer,
                            Op::Description(s.group_description_input.text.clone()),
                        )
                    })
                    .build(k)
                    .key("save-group-description"),
            );
    }
    if c.group_member && (admin || c.permission_add == "EVERY_MEMBER") {
        let peer = c.id.clone();
        controls = controls.child(
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .child(
                    TextInput::new(&s.group_member_input)
                        .label("Add a member")
                        .placeholder("Contact name, number or UUID")
                        .on_edit(|s: &mut State| &mut s.group_member_input)
                        .build(k)
                        .grow(1.)
                        .min_w(0.)
                        .key("group-add-recipient"),
                )
                .child(
                    Button::new("Add")
                        .disabled(busy || s.group_member_input.text.trim().is_empty())
                        .on_press(move |s: &mut State| {
                            s.group_operation(&peer, Op::Add(s.group_member_input.text.clone()))
                        })
                        .build(k)
                        .key("group-add-member"),
                ),
        );
    }
    if admin {
        controls = controls.child(label("Group permissions", 13., 600., t.fg));
        for (title, key, value) in [
            ("Add members", "setPermissionAddMember", &c.permission_add),
            (
                "Edit group details",
                "setPermissionEditDetails",
                &c.permission_edit,
            ),
            (
                "Send messages",
                "setPermissionSendMessages",
                &c.permission_send,
            ),
        ] {
            controls = controls.child(
                col()
                    .w_full()
                    .gap(6.)
                    .child(label(title, 12., 400., t.muted))
                    .child(
                        row()
                            .gap(8.)
                            .child(group_op_button(
                                "All members",
                                &c.id,
                                Op::Permission(key, "every-member"),
                                busy || value == "EVERY_MEMBER",
                                false,
                                k,
                            ))
                            .child(group_op_button(
                                "Admins",
                                &c.id,
                                Op::Permission(key, "only-admins"),
                                busy || value == "ONLY_ADMINS",
                                false,
                                k,
                            )),
                    ),
            );
        }
        controls = controls.child(label("Group link", 13., 600., t.fg)).child(
            row()
                .gap(6.)
                .flex_wrap()
                .child(group_op_button(
                    "Enable",
                    &c.id,
                    Op::Link("enabled"),
                    busy,
                    false,
                    k,
                ))
                .child(group_op_button(
                    "Require approval",
                    &c.id,
                    Op::Link("enabled-with-approval"),
                    busy,
                    false,
                    k,
                ))
                .child(group_op_button(
                    "Disable",
                    &c.id,
                    Op::Link("disabled"),
                    busy,
                    true,
                    k,
                ))
                .child(group_op_button(
                    "Reset link",
                    &c.id,
                    Op::ResetLink,
                    busy,
                    true,
                    k,
                )),
        );
    }
    if c.group_member {
        let only_admin = admin && c.group_admins.len() == 1 && c.members.len() > 1;
        controls = controls.child(
            group_op_button("Leave group", &c.id, Op::Leave, busy || only_admin, true, k)
                .key("leave-group"),
        );
        if only_admin {
            controls = controls.child(
                label(
                    "Make another member an admin before leaving the group.",
                    11.,
                    400.,
                    t.muted,
                )
                .wrap(),
            );
        }
    }
    controls
}
fn group_dialogs(s: &State, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    use crate::model::GroupOperation as Op;
    if s.new_group {
        let busy = s
            .pending
            .values()
            .any(|p| matches!(p, crate::Pending::CreatedGroup));
        let mut content = col()
            .w_full()
            .gap(12.)
            .padding(16.)
            .child(
                TextInput::new(&s.group_name_input)
                    .label("Group name")
                    .placeholder("Group name")
                    .on_edit(|s: &mut State| &mut s.group_name_input)
                    .build(k)
                    .key("new-group-name"),
            )
            .child(label(
                format!("{} members selected", s.group_participants.len()),
                12.,
                500.,
                t.muted,
            ))
            .child(
                row()
                    .w_full()
                    .gap(8.)
                    .center_y()
                    .child(
                        TextInput::new(&s.group_member_input)
                            .label("Find or add a member")
                            .placeholder("Contact name, number or UUID")
                            .on_edit(|s: &mut State| &mut s.group_member_input)
                            .on_submit(|s: &mut State, _| s.add_group_participant())
                            .build(k)
                            .grow(1.)
                            .min_w(0.)
                            .key("new-group-recipient"),
                    )
                    .child(
                        Button::new("Add")
                            .disabled(busy || s.group_member_input.text.trim().is_empty())
                            .on_press(|s: &mut State| s.add_group_participant())
                            .build(k),
                    ),
            );
        let mut selected = row().w_full().gap(6.).flex_wrap();
        for id in &s.group_participants {
            let person = id.clone();
            selected = selected.child(
                Button::new(format!("{} ×", s.display_name(id)))
                    .on_press(move |s: &mut State| s.toggle_group_participant(&person))
                    .build(k),
            );
        }
        if !s.group_participants.is_empty() {
            content = content.child(
                col()
                    .w_full()
                    .max_h(64.)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(selected),
            );
        }
        let mut contacts: Vec<_> = s
            .conversations
            .iter()
            .filter(|c| !c.group && !c.blocked && !s.account.as_ref().is_some_and(|a| c.matches(a)))
            .collect();
        contacts.sort_by_key(|c| c.name.to_lowercase());
        let query = s.group_member_input.text.to_lowercase();
        let mut choices = col().w_full().gap(4.);
        for c in contacts
            .into_iter()
            .filter(|c| {
                query.is_empty() || c.name.to_lowercase().contains(&query) || c.id.contains(&query)
            })
            .take(100)
        {
            let id = c.id.clone();
            let selected = s.group_participants.contains(&id);
            choices = choices.child(
                row()
                    .w_full()
                    .h(40.)
                    .px(8.)
                    .gap(10.)
                    .center_y()
                    .radius(6.)
                    .fill(if selected { t.selected } else { t.bg })
                    .role("button")
                    .label(format!(
                        "{}{}",
                        if selected { "Selected: " } else { "Select: " },
                        c.name
                    ))
                    .focusable(true)
                    .on_press(move |s: &mut State| s.toggle_group_participant(&id))
                    .child(avatar(&c.name, 28., 0))
                    .child(
                        label(&c.name, 13., 500., t.fg)
                            .truncate()
                            .grow(1.)
                            .min_w(0.),
                    )
                    .child(label(if selected { "✓" } else { "+" }, 16., 500., t.muted)),
            );
        }
        content = content
            .child(
                col()
                    .w_full()
                    .max_h(160.)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(choices),
            )
            .child(
                row()
                    .w_full()
                    .justify_end()
                    .gap(8.)
                    .child(
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .disabled(busy)
                            .on_press(|s: &mut State| s.new_group = false)
                            .build(k),
                    )
                    .child(
                        Button::new(if busy { "Creating…" } else { "Create group" })
                            .disabled(
                                s.demo
                                    || busy
                                    || s.group_name_input.text.trim().is_empty()
                                    || s.group_participants.is_empty(),
                            )
                            .on_press(|s: &mut State| s.create_group())
                            .build(k)
                            .key("create-group"),
                    ),
            );
        Dialog::new("new-group", "New group")
            .fit()
            .width(460.)
            .open(true)
            .on_close(|s: &mut State| s.new_group = false)
            .child(content)
            .build(k, layer);
    }
    if let Some((peer, person)) = &s.group_member_panel
        && let Some(c) = s.conversations.iter().find(|c| &c.id == peer)
    {
        let self_member = s.account.as_ref() == Some(person);
        let admin = s
            .account
            .as_ref()
            .is_some_and(|a| c.group_admins.contains(a));
        let busy = s.demo
            || s.pending
                .values()
                .any(|p| matches!(p,crate::Pending::GroupUpdate(id) if id==peer));
        let mut content = col().padding(16.).gap(12.).w_full().child(label(
            s.display_name(person),
            18.,
            600.,
            t.fg,
        ));
        if !self_member {
            let id = person.clone();
            content = content.child(
                Button::new("View safety number")
                    .on_press(move |s: &mut State| {
                        s.group_member_panel = None;
                        s.open_identity(&id);
                    })
                    .build(k),
            );
        }
        if admin && !self_member {
            if c.group_banned.contains(person) {
                content = content.child(group_op_button(
                    "Unban member",
                    peer,
                    Op::Unban(person.clone()),
                    busy,
                    true,
                    k,
                ));
            } else if c.group_requesting.contains(person) {
                content = content
                    .child(group_op_button(
                        "Approve request",
                        peer,
                        Op::Add(person.clone()),
                        busy,
                        false,
                        k,
                    ))
                    .child(group_op_button(
                        "Deny request",
                        peer,
                        Op::Remove(person.clone()),
                        busy,
                        true,
                        k,
                    ));
            } else if c.group_pending.contains(person) {
                content = content.child(group_op_button(
                    "Revoke invitation",
                    peer,
                    Op::Remove(person.clone()),
                    busy,
                    true,
                    k,
                ));
            } else {
                content = content
                    .child(if c.group_admins.contains(person) {
                        group_op_button(
                            "Remove admin role",
                            peer,
                            Op::Demote(person.clone()),
                            busy,
                            true,
                            k,
                        )
                    } else {
                        group_op_button(
                            "Make admin",
                            peer,
                            Op::Promote(person.clone()),
                            busy,
                            true,
                            k,
                        )
                    })
                    .child(group_op_button(
                        "Remove from group",
                        peer,
                        Op::Remove(person.clone()),
                        busy,
                        true,
                        k,
                    ))
                    .child(group_op_button(
                        "Ban from group",
                        peer,
                        Op::Ban(person.clone()),
                        busy,
                        true,
                        k,
                    ));
            }
        }
        Dialog::new("group-member", "Group member")
            .fit()
            .width(400.)
            .open(true)
            .on_close(|s: &mut State| s.group_member_panel = None)
            .child(content)
            .build(k, layer);
    }
    if let Some((peer, op)) = &s.group_confirmation {
        let title = match op {
            Op::Leave => "Leave this group?",
            Op::Remove(_) => "Remove this member?",
            Op::Ban(_) => "Ban this member?",
            Op::Unban(_) => "Unban this member?",
            Op::Promote(_) => "Make this member an admin?",
            Op::Demote(_) => "Remove this member’s admin role?",
            Op::ResetLink => "Reset the group link?",
            Op::Link(_) => "Disable the group link?",
            _ => "Apply this group change?",
        };
        let peer = peer.clone();
        let op = op.clone();
        let content = col()
            .padding(16.)
            .gap(18.)
            .child(label(title, 15., 500., t.fg).wrap())
            .child(
                row()
                    .justify_end()
                    .gap(8.)
                    .child(
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .on_press(|s: &mut State| s.group_confirmation = None)
                            .build(k),
                    )
                    .child(
                        Button::new("Confirm")
                            .on_press(move |s: &mut State| s.group_operation(&peer, op.clone()))
                            .build(k)
                            .key("confirm-group-change"),
                    ),
            );
        Dialog::new("group-confirm", "Confirm group change")
            .fit()
            .width(400.)
            .open(true)
            .on_close(|s: &mut State| s.group_confirmation = None)
            .child(content)
            .build(k, layer);
    }
}
