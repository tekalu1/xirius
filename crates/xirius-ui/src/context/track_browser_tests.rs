//! Real indexed presets feed the same menu builders used by window input.
use super::{
    Act, Run,
    browser::BrowseAct,
    tests::{all_items, studio},
};
use crate::{
    arrange::probe::track,
    browser::{BRow, RailTab},
};
use serde_json::json;
use xirius_core::track_browser::Category;

#[test]
fn indexed_presets_keep_payloads_and_use_shared_categories() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    assert!(s.set_instrument(t, &crate::instrument::Pick::built_in("Sampler")));
    assert!(s.run_command("plugin.add", json!({"track":t.to_string(),"name":"EQ"})));
    let fx = s.project().tracks[&t].inserts.last().unwrap().id;
    for (kind, name, include_fx) in [
        ("instrument", "Catalog plain", false),
        ("instrument", "Catalog room", true),
        ("fx", "Catalog EQ", false),
        ("fx-chain", "Catalog chain", false),
        ("track", "Catalog track", false),
    ] {
        let mut args =
            json!({"track":t.to_string(),"kind":kind,"name":name,"include_fx":include_fx});
        if kind == "fx" {
            args["of"] = json!(fx.to_string());
        }
        assert!(s.run_command("browser.preset.save", args), "{:?}", s.error);
    }
    assert!(s.run_command(
        "rack.layer.add",
        json!({"track":t.to_string(),"name":"Sampler"})
    ));
    assert!(s.run_command(
        "browser.preset.save",
        json!({"track":t.to_string(),"kind":"multi-instrument","name":"Catalog multi"})
    ));
    s.refresh_presets();
    assert!(s.run_command("browser.preset.list", json!({})));
    let entries = s.browser.track_library.clone().unwrap();
    let presets: Vec<_> = entries
        .iter()
        .filter(|e| {
            e.name.starts_with("Catalog ") || (e.source == "Factory" && e.category == Category::Fx)
        })
        .collect();
    assert!(presets.iter().any(|e| e.source == "Factory"));
    for entry in presets {
        s.browser.open = true;
        s.browser.tab = if entry.category.tab() == "effects" {
            RailTab::Effects
        } else {
            RailTab::Instruments
        };
        s.browser.track_kind = entry.category == Category::Track;
        s.browser.search = entry.name.clone();
        s.browser.scroll = 0.0;
        for favourite in [false, true] {
            if favourite {
                s.browser.favourites.insert(entry.id.clone());
            } else {
                s.browser.favourites.remove(&entry.id);
            }
            let layout = s.browser.layout(g.panel);
            let placed = layout
                .rows
                .iter()
                .find(|r| matches!(&r.row, BRow::Preset { device, .. } if *device == entry.id))
                .unwrap();
            let rect = s.browser.row_rect(&layout, placed);
            assert!(s.lib_context(&g, rect.x + 80.0, rect.y + rect.h / 2.0));
            let roots: Vec<_> = s
                .ctx
                .as_ref()
                .unwrap()
                .items
                .iter()
                .map(|i| i.label.as_str())
                .collect();
            assert_eq!(
                roots,
                [
                    if entry.category == Category::Track {
                        "トラック"
                    } else {
                        "機器"
                    },
                    "表示"
                ]
            );
            let items = all_items(&s);
            assert!(items.iter().any(|i| matches!(&i.run, Run::Do(Act::Browse(BrowseAct::LoadPreset(id, name))) if *id == entry.id && *name == entry.name)));
            assert!(items.iter().any(|i| matches!(&i.run, Run::Do(Act::Browse(BrowseAct::CopyPreset(id))) if *id == entry.id)));
            assert!(items.iter().any(|i| matches!(&i.run, Run::Do(Act::Browse(BrowseAct::FavouritePreset(id, on))) if *id == entry.id && *on != favourite) && i.on == favourite));
            if matches!(entry.category, Category::Instrument | Category::Fx) {
                assert!(items.iter().any(|i| matches!(&i.run, Run::Do(Act::Browse(BrowseAct::FactoryRestore(Some(uid)))) if *uid == entry.device)));
            }
        }
    }
    for entry in entries.iter().filter(|e| e.file.is_none()) {
        s.browser.tab = if entry.category == Category::Fx {
            RailTab::Effects
        } else {
            RailTab::Instruments
        };
        s.browser.track_kind = false;
        s.browser.search = entry.name.clone();
        s.browser.scroll = 0.0;
        let layout = s.browser.layout(g.panel);
        let placed = layout
            .rows
            .iter()
            .find(|r| matches!(&r.row, BRow::Item { item, .. } if item.name == entry.name))
            .unwrap();
        let rect = s.browser.row_rect(&layout, placed);
        assert!(s.lib_context(&g, rect.x + 80.0, rect.y + rect.h / 2.0));
        assert!(all_items(&s).iter().any(|i| i.label == "隠す"));
        assert!(
            all_items(&s)
                .iter()
                .any(|i| i.label == "ファイルの場所を表示" && !i.enabled)
        );
        if matches!(
            placed.row,
            BRow::Item {
                presets: Some(_),
                ..
            }
        ) {
            let before = s.ctx.as_ref().unwrap().items.clone();
            let arrow_x = (0..80)
                .map(|x| rect.x + x as f32)
                .find(|&x| {
                    matches!(
                        s.browser.hit(&layout, x, rect.y + rect.h / 2.0),
                        Some(crate::browser::BrowserHit::Disclose(_))
                    )
                })
                .unwrap();
            assert!(s.lib_context(&g, arrow_x, rect.y + rect.h / 2.0));
            assert_eq!(s.ctx.as_ref().unwrap().items, before);
        }
    }
    s.browser.search = "no matching preset".into();
    for scanning in [false, true] {
        s.browser.scanning = scanning;
        let layout = s.browser.layout(g.panel);
        assert!(s.lib_context(&g, layout.list.x + 30.0, layout.list.bottom() - 5.0));
        let items = all_items(&s);
        assert!(items.iter().any(
            |i| matches!(&i.run, Run::Do(Act::Browse(BrowseAct::Rescan))) && i.enabled != scanning
        ));
        assert!(items.iter().any(|i| matches!(
            &i.run,
            Run::Do(Act::Browse(BrowseAct::FactoryRestore(None)))
        )));
        assert!(
            items.iter().any(
                |i| matches!(&i.run, Run::Do(Act::Browse(BrowseAct::Groups(true)))) && !i.enabled
            )
        );
    }
}
