// The pages of channel settings: Overview, Permissions, Invites and Webhooks, each a column of rows.
use super::*;

impl Waffle {
    pub(super) fn overview_page(&self, channel: &Channel, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let props = view.draft.props.clone();
        let voice = matches!(channel.raw_type, 2 | 13);
        let category = channel.kind == ChannelKind::Category;
        let forum = matches!(channel.raw_type, 15 | 16);
        let mut out = Vec::new();

        let name_focus = view.name.read(cx).focus_handle(cx);
        let name = ui::field("Channel name", cx).child(ui::text_field("cs-name", Input::new(&view.name).appearance(false), &name_focus).leading(Icon::new(channel_glyph(channel)).size(ui::ICON_MD)));
        let topic = (!voice && !category).then(|| {
            let focus = view.topic.read(cx).focus_handle(cx);
            let count = view.topic.read(cx).value().chars().count();
            let cap = if forum { 4096 } else { 1024 };
            ui::field(if forum { "Post guidelines" } else { "Topic" }, cx)
                .child(ui::text_area("cs-topic", Textarea::new(&view.topic).appearance(false).px_0().text_size(ui::TEXT_SM), &focus).error(count > cap))
                .child(ui::field_hint("Shows in the channel header.", Some(format!("{count} / {cap}").into()), cx))
        });
        out.push(ui::settings_group(if category || voice { "Name" } else { "Name and topic" }, cx).child(ui::settings_card(cx).p_4().gap_4().child(name).children(topic)).into_any_element());
        if category {
            return out;
        }

        if voice {
            let cap = self.guilds.iter().find(|g| g.id == view.guild_id).map_or(96_000, |g| bitrate_cap(g.premium_tier, channel.raw_type == 13));
            let bitrate = props.bitrate.clamp(8_000, cap);
            let fraction = (bitrate - 8_000) as f32 / (cap - 8_000) as f32;
            let bitrate_row = ui::settings_row("cs-bitrate", false, cx)
                .flex_col()
                .items_stretch()
                .gap_3()
                .child(ui::row().items_start().gap_4().child(ui::settings_text("Bitrate", Some("Higher sounds better and costs slow connections more"), cx)).child(value_text(format!("{} kbps", bitrate / 1000), cx)))
                .child(self.range("cs-bitrate".into(), fraction, crate::settings::Fill::Accent, move |this, f, cx| {
                    let kbps = (8. + f.clamp(0., 1.) * (cap - 8_000) as f32 / 1000.).round() as u32;
                    this.edit_draft(cx, |d| d.props.bitrate = kbps * 1000);
                }, cx));
            let limit_max = if channel.raw_type == 13 { 10_000 } else { 99 };
            let limit = props.user_limit.min(limit_max);
            let limit_row = ui::settings_row("cs-limit", true, cx)
                .flex_col()
                .items_stretch()
                .gap_3()
                .child(ui::row().items_start().gap_4().child(ui::settings_text("User limit", Some("Members with Move members can still join"), cx)).child(value_text(if limit == 0 { "No limit".into() } else { limit.to_string() }, cx)))
                .child(self.range("cs-limit".into(), limit as f32 / limit_max as f32, crate::settings::Fill::Accent, move |this, f, cx| {
                    let n = (f.clamp(0., 1.) * limit_max as f32).round() as u32;
                    this.edit_draft(cx, |d| d.props.user_limit = n);
                }, cx));
            let quality = ui::settings_row("cs-quality", true, cx)
                .child(ui::settings_text("Video quality", Some("720p sends full quality to everyone watching"), cx))
                .child(ui::segmented_live("cs-quality-seg", &["Auto", "720p"], props.video_full as usize, {
                    let this = cx.entity();
                    move |i, _, cx| this.update(cx, |this, cx| this.edit_draft(cx, |d| d.props.video_full = i == 1))
                }, window, cx));
            let mut regions = vec![(String::new(), "Automatic".to_owned())];
            regions.extend(view.regions.iter().cloned());
            let chosen = props.rtc_region.clone().unwrap_or_default();
            let region_name = regions.iter().find(|(id, _)| *id == chosen).map_or(chosen.clone(), |(_, n)| n.clone());
            let region = ui::settings_row("cs-region", true, cx)
                .child(ui::settings_text("Region", Some("Where the voice server runs"), cx))
                .child(div().w(px(SELECT_WIDTH)).flex_none().child(self.cs_select(Select::Region, &regions, &chosen, region_name, cx)));
            out.push(ui::settings_group("Sound and video", cx).child(ui::settings_card(cx).child(bitrate_row).child(limit_row).child(quality).child(region)).into_any_element());
        }

        let slow_options: Vec<(String, String)> = SLOWMODE.iter().map(|(s, l)| (s.to_string(), (*l).to_owned())).collect();
        let slow_name = SLOWMODE.iter().find(|(s, _)| *s == props.slowmode).map_or(format!("{} seconds", props.slowmode), |(_, l)| (*l).to_owned());
        let slowmode = ui::settings_row("cs-slowmode", false, cx)
            .child(ui::settings_text("Slowmode", Some("Each member sends one message and starts one thread per interval"), cx))
            .child(div().w(px(SELECT_WIDTH)).flex_none().child(self.cs_select(Select::Slowmode, &slow_options, &props.slowmode.to_string(), slow_name, cx)));
        let nsfw = ui::settings_switch("cs-nsfw", "Age-restricted", "People confirm they're over 18 before they see it", props.nsfw, true, window, cx).on_click(cx.listener(|this, _, _, cx| this.edit_draft(cx, |d| d.props.nsfw = !d.props.nsfw)));
        let archive = (!voice).then(|| {
            let options: Vec<(String, String)> = ARCHIVE.iter().map(|(m, l)| (m.to_string(), (*l).to_owned())).collect();
            let current = if props.archive_after == 0 { 4320 } else { props.archive_after };
            let label = ARCHIVE.iter().find(|(m, _)| *m == current).map_or("3 days", |(_, l)| l).to_owned();
            ui::settings_row("cs-archive", true, cx)
                .child(ui::settings_text("Hide threads after", Some("New threads close after this long without activity"), cx))
                .child(div().w(px(SELECT_WIDTH)).flex_none().child(self.cs_select(Select::Archive, &options, &current.to_string(), label, cx)))
        });
        let announcement = self.announcement_toggle(channel, view.draft.announcement, window, cx);
        let chat = ui::settings_card(cx).child(slowmode).child(nsfw).children(announcement.0).children(archive);
        out.push(ui::settings_group(if voice { "Text chat" } else { "Chat" }, cx).child(chat).child(ui::settings_caption("Members with Bypass slowmode aren't slowed down.", cx)).children(announcement.1).into_any_element());
        out
    }

    // Discord's Announcement Channel switch and its profile notice.
    fn announcement_toggle(&self, channel: &Channel, on: bool, window: &mut Window, cx: &mut Context<Self>) -> (Option<AnyElement>, Option<AnyElement>) {
        let shown = self.guilds.iter().find(|g| g.channel(&channel.id).is_some()).is_some_and(|g| crate::announcements::offers_announcement_switch(g, channel));
        if !shown {
            return (None, None);
        }
        let row = ui::settings_switch("cs-announcement", "Announcement Channel", "Users can Follow this channel in their own servers. Posts you \"Publish\" from this channel will appear in those servers.", on, true, window, cx)
            .on_click(cx.listener(|this, _, _, cx| this.edit_draft(cx, |d| d.announcement = !d.announcement)));
        let notice = ui::callout(
            Notice::Info,
            Glyph::Info,
            "By creating an announcement channel, your server profile will be visible to others outside your server members.",
            "You can control Profile privacy in Server Settings > Profile.",
            cx,
        );
        (Some(row.into_any_element()), Some(div().pt_2().child(notice).into_any_element()))
    }

    pub(super) fn permissions_page(&self, channel: &Channel, guild_id: &str, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return Vec::new() };
        let overwrites = view.draft.overwrites.clone();
        let mut out = Vec::new();

        let parent = guild.channels.iter().find(|c| Some(&c.id) == channel.parent_id.as_ref());
        if let Some(parent) = parent {
            let notice = if synced(&overwrites, &parent.overwrites) {
                ui::callout(Notice::Info, Glyph::Link, format!("Synced with {}", parent.name), "Changes to the category apply here too.", cx)
            } else {
                let parent_overwrites = parent.overwrites.clone();
                let sync = ui::button("cs-sync", Tone::Secondary, cx).small().child(format!("Sync with {}", parent.name)).on_click(cx.listener(move |this, _, _, cx| {
                    let next = parent_overwrites.clone();
                    this.edit_draft(cx, |d| d.overwrites = next);
                }));
                ui::callout(Notice::Warning, Glyph::Link, format!("Not synced with {}", parent.name), "This channel's permissions differ from its category.", cx).child(div().flex_none().child(sync))
            };
            out.push(notice.into_any_element());
        }

        let (everyone, raw_type) = (guild.id.clone(), channel.raw_type);
        let private_on = private(&overwrites, &everyone);
        let private_row = ui::settings_switch("cs-private", "Private channel", "Only the roles and members you add can see it", private_on, false, window, cx).on_click(cx.listener(move |this, _, _, cx| {
            let everyone = everyone.clone();
            this.edit_draft(cx, |d| set_private(&mut d.overwrites, &everyone, raw_type, !private_on));
        }));
        out.push(ui::settings_card(cx).child(private_row).into_any_element());

        // Targets: @everyone first, then roles highest first, then members.
        let role = |id: &str| guild.roles.iter().find(|r| r.id == id);
        // Discord always lists @everyone, overwrite or not; editing it creates one.
        let mut shown = overwrites.clone();
        if !shown.iter().any(|o| o.id == guild.id) {
            shown.push(Overwrite { id: guild.id.clone(), ..Default::default() });
        }
        let mut targets: Vec<&Overwrite> = shown.iter().collect();
        targets.sort_by_key(|o| (o.member, if o.id == guild.id { i64::MIN } else { -role(&o.id).map_or(0, |r| r.position) }));
        let selected = view.target.clone().filter(|t| shown.iter().any(|o| &o.id == t)).or_else(|| targets.first().map(|o| o.id.clone()));
        let name_of = |o: &Overwrite| -> String {
            match role(&o.id) {
                Some(r) => r.name.clone(),
                None if o.member => self.overwrite_member_name(guild_id, &o.id),
                None => "Unknown role".into(),
            }
        };
        let dark = cx.theme().is_dark();
        let rows: Vec<AnyElement> = targets
            .iter()
            .map(|o| {
                let id = o.id.clone();
                let color = role(&o.id).filter(|r| r.color != 0).map_or(palette::faint(cx), |r| palette::role_ink(r.color, dark));
                let mark = if o.member { div().child(Icon::new(Glyph::User).size(ui::ICON_MD)).text_color(palette::muted(cx)) } else { div().size_3().m_0p5().rounded_full().bg(color) }; // lint-allow(raw-margin): centres the 12px role dot on the 16px icon column, an optical nudge
                ui::menu_row(SharedString::from(format!("cs-target-{id}")), cx)
                    .toggled(selected.as_deref() == Some(o.id.as_str()), cx)
                    .child(mark.flex_none())
                    .child(ui::line(name_of(o)).flex_1())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(view) = &mut this.channel_settings {
                            view.target = Some(id.clone());
                        }
                        cx.notify(); // lint-allow(global-notify): the dialog is built by render_root, not by a column
                    }))
                    .into_any_element()
            })
            .collect();
        let addable: Vec<(String, String)> = {
            let mut roles: Vec<_> = guild.roles.iter().filter(|r| !overwrites.iter().any(|o| o.id == r.id)).collect();
            roles.sort_by_key(|r| -r.position);
            roles.into_iter().map(|r| (r.id.clone(), r.name.clone())).collect()
        };
        let add = (!addable.is_empty()).then(|| div().pt_2().child(self.cs_select(Select::AddRole, &addable, "", "Add a role".into(), cx)));
        let list = ui::col()
            .w(px(TARGET_LIST_WIDTH))
            .flex_none()
            .gap_0p5()
            .child(div().px_1().pb_1().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Roles and members"))
            .children(rows)
            .children(add);

        let editor = selected.and_then(|id| shown.iter().find(|o| o.id == id).cloned()).map(|target| {
            let who = name_of(&target);
            let groups: Vec<AnyElement> = permission_groups(channel.raw_type)
                .into_iter()
                .map(|(title, perms)| {
                    let rows = perms.iter().enumerate().map(|(i, &(bit, label))| {
                        let (id, member) = (target.id.clone(), target.member);
                        ui::settings_row(SharedString::from(format!("cs-perm-{title}-{bit}")), i > 0, cx)
                            .child(ui::settings_text(label, None::<&str>, cx))
                            .child(ui::tri_state(SharedString::from(format!("cs-tri-{title}-{bit}")), tri(&overwrites, &target.id, bit), {
                                let this = cx.entity();
                                move |value, _, cx| {
                                    let id = id.clone();
                                    this.update(cx, |this, cx| this.edit_draft(cx, |d| set_tri(&mut d.overwrites, &id, member, bit, value)));
                                }
                            }, cx))
                    });
                    ui::settings_group(title, cx).child(ui::settings_card(cx).children(rows)).into_any_element()
                })
                .collect();
            let remove_id = target.id.clone();
            let remove = (target.id != guild.id).then(|| {
                ui::button("cs-remove-target", Tone::Ghost, cx).small().child(Icon::new(Glyph::Trash).size(ui::ICON_MD)).child(format!("Remove {who}")).on_click(cx.listener(move |this, _, _, cx| {
                    let id = remove_id.clone();
                    this.edit_draft(cx, |d| d.overwrites.retain(|o| o.id != id));
                }))
            });
            ui::col().min_h_0().flex_1().gap_6().children(groups).child(ui::row().items_stretch().justify_end().children(remove))
        });
        out.push(ui::row().items_start().gap_6().child(list).children(editor).into_any_element());
        out
    }

    fn overwrite_member_name(&self, guild_id: &str, user_id: &str) -> String {
        self.members.get(guild_id).and_then(|m| m.get(user_id)).map_or_else(|| "Member".into(), |u| u.name.clone())
    }

    pub(super) fn invites_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let invites = match &view.invites {
            Load::Loading => return vec![loading_line("Loading invites…", cx)],
            Load::Failed(error) => return vec![ui::callout(Notice::Danger, Glyph::WarningCircle, "Couldn't load invites", error.clone(), cx).into_any_element()],
            Load::Ready(list) if list.is_empty() => return vec![ui::settings_caption("No invites to this channel yet. Invite people from the channel's menu.", cx).into_any_element()],
            Load::Ready(list) => list.clone(),
        };
        let now = chrono::Utc::now();
        let rows = invites.into_iter().enumerate().map(|(i, invite)| {
            let uses = if invite.max_uses == 0 { invite.uses.to_string() } else { format!("{} of {}", invite.uses, invite.max_uses) };
            let code = invite.code.clone();
            ui::settings_row(SharedString::from(format!("cs-invite-{}", invite.code)), i > 0, cx)
                .child(ui::settings_text(invite.code.clone(), Some(invite.inviter.clone()), cx))
                .child(div().w_20().flex_none().text_color(palette::text(cx)).child(uses))
                .child(div().w(px(EXPIRES_WIDTH)).flex_none().text_color(palette::muted(cx)).child(expires_label(invite.expires_at.as_deref(), now)))
                .child(ui::button(SharedString::from(format!("cs-revoke-{}", invite.code)), Tone::Secondary, cx).small().child("Revoke").on_click(cx.listener(move |this, _, _, cx| {
                    let code = code.clone();
                    this.rest(Method::DELETE, format!("/invites/{code}"), None, move |this, cx| {
                        if let Some(Load::Ready(list)) = this.channel_settings.as_mut().map(|v| &mut v.invites) {
                            list.retain(|i| i.code != code);
                        }
                        cx.notify(); // lint-allow(global-notify): the dialog is built by render_root, not by a column
                    }, cx);
                })))
        });
        vec![ui::settings_card(cx).children(rows).into_any_element()]
    }

    pub(super) fn webhooks_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let hooks = match &view.webhooks {
            Load::Loading => return vec![loading_line("Loading webhooks…", cx)],
            Load::Failed(error) => return vec![ui::callout(Notice::Danger, Glyph::WarningCircle, "Couldn't load webhooks", error.clone(), cx).into_any_element()],
            Load::Ready(list) if list.is_empty() => return vec![ui::settings_caption("No webhooks post here. Creating one stays in Discord for now.", cx).into_any_element()],
            Load::Ready(list) => list.clone(),
        };
        let rows = hooks.into_iter().enumerate().map(|(i, hook)| {
            let (id, url) = (hook.id.clone(), hook.url.clone());
            let copy = url.map(|url| ui::button(SharedString::from(format!("cs-hook-copy-{}", hook.id)), Tone::Secondary, cx).small().child("Copy URL").on_click(cx.listener(move |this, _, _, cx| this.copy_text(url.clone(), cx))));
            ui::settings_row(SharedString::from(format!("cs-hook-{}", hook.id)), i > 0, cx)
                .child(ui::icon_tile(Glyph::AppWindow, palette::selected(cx), palette::strong(cx)).flex_none())
                .child(ui::settings_text(hook.name.clone(), hook.by.as_ref().map(|by| format!("Made by {by}")), cx))
                .children(copy)
                .child(ui::icon_button(SharedString::from(format!("cs-hook-del-{}", hook.id)), Glyph::Trash, format!("Delete {}", hook.name), cx).on_click(cx.listener(move |this, _, _, cx| {
                    let id = id.clone();
                    this.rest(Method::DELETE, format!("/webhooks/{id}"), None, move |this, cx| {
                        if let Some(Load::Ready(list)) = this.channel_settings.as_mut().map(|v| &mut v.webhooks) {
                            list.retain(|h| h.id != id);
                        }
                        cx.notify(); // lint-allow(global-notify): the dialog is built by render_root, not by a column
                    }, cx);
                })))
        });
        vec![ui::settings_group("Webhooks", cx).child(ui::settings_card(cx).children(rows)).child(ui::settings_caption("Creating webhooks and following channels stay in Discord for now.", cx)).into_any_element()]
    }
}
