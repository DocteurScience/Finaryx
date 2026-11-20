//! "Ajouter une opération" / "Modifier la saisie": an operation typed by
//! hand (`finaryx_core::manual`). Date, label, amount with its direction,
//! category (proposed from the label like for an import) and, when adding,
//! the account.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use chrono::{Datelike, NaiveDate};
use finaryx_core::categories;
use finaryx_core::dates;
use finaryx_core::manual::{self, ManualOperation};
use finaryx_core::models::Id;
use finaryx_core::money::{self, Currency, Sign};
use finaryx_core::repo;
use gettextrs::gettext;
use gtk::glib;

use crate::window::FinaryxWindow;

/// Opens the dialog: `txn` = `Some(id)` to change a manual operation,
/// `None` to add one (to `account` if given, else the selected account).
pub fn present(window: &FinaryxWindow, account: Option<Id>, txn: Option<Id>) {
    let editing = txn.and_then(|id| window.read(|c| manual::get(c, id)).flatten());
    if txn.is_some() && editing.is_none() {
        return;
    }
    let accounts = window
        .read(|c| repo::list_accounts(c, false))
        .unwrap_or_default();
    if accounts.is_empty() {
        window.toast(&gettext("Ajoutez d’abord un compte."));
        return;
    }
    let category_list = window
        .read(categories::list_for_display)
        .unwrap_or_default();

    // Account: fixed when changing an operation.
    let account_row = adw::ComboRow::builder()
        .title(gettext("Compte"))
        .model(&gtk::StringList::new(
            &accounts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        ))
        .build();
    let wanted = editing
        .as_ref()
        .map(|(a, _)| *a)
        .or(account)
        .or_else(|| window.selected_account());
    if let Some(i) = wanted.and_then(|id| accounts.iter().position(|a| a.id == id)) {
        account_row.set_selected(i as u32);
    }
    account_row.set_sensitive(editing.is_none());

    let initial = editing.as_ref().map(|(_, op)| op.clone());
    let date = Rc::new(Cell::new(
        initial
            .as_ref()
            .map_or_else(|| window.today(), |op| op.booked_on),
    ));
    let date_row = adw::ActionRow::builder()
        .title(gettext("Date"))
        .subtitle(dates::day_month_year_long(date.get()))
        .build();
    let calendar = gtk::Calendar::new();
    if let Ok(d) = glib::DateTime::from_local(
        date.get().year(),
        date.get().month() as i32,
        date.get().day() as i32,
        12,
        0,
        0.0,
    ) {
        calendar.select_day(&d);
    }
    let popover = gtk::Popover::builder().child(&calendar).build();
    let pick = gtk::MenuButton::builder()
        .icon_name("x-office-calendar-symbolic")
        .tooltip_text(gettext("Choisir la date"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .popover(&popover)
        .build();
    date_row.add_suffix(&pick);
    date_row.set_activatable_widget(Some(&pick));
    {
        let (date, date_row, popover) = (date.clone(), date_row.clone(), popover.clone());
        calendar.connect_day_selected(move |c| {
            let d = c.date();
            if let Some(day) =
                NaiveDate::from_ymd_opt(d.year(), d.month() as u32, d.day_of_month() as u32)
            {
                date.set(day);
                date_row.set_subtitle(&dates::day_month_year_long(day));
            }
            popover.popdown();
        });
    }

    let label = adw::EntryRow::builder()
        .title(gettext("Libellé"))
        .text(initial.as_ref().map_or("", |op| op.label.as_str()))
        .build();

    // Direction: spending by default.
    let spending = gtk::ToggleButton::with_label(&gettext("Dépense"));
    let income = gtk::ToggleButton::with_label(&gettext("Entrée"));
    income.set_group(Some(&spending));
    let incoming = initial.as_ref().is_some_and(|op| op.amount_cents > 0);
    income.set_active(incoming);
    spending.set_active(!incoming);
    let direction = gtk::Box::builder()
        .css_classes(["linked"])
        .valign(gtk::Align::Center)
        .build();
    direction.append(&spending);
    direction.append(&income);
    let direction_row = adw::ActionRow::builder().title(gettext("Sens")).build();
    direction_row.add_suffix(&direction);

    let currency_of = {
        let accounts = accounts.clone();
        let account_row = account_row.clone();
        move || {
            accounts
                .get(account_row.selected() as usize)
                .and_then(|a| Currency::from_code(&a.currency))
                .unwrap_or(Currency::EUR)
        }
    };
    let amount_text = initial.as_ref().map_or_else(String::new, |op| {
        let currency = currency_of();
        // Without the symbol nor the sign, so that the text reads back.
        money::format_amount(op.amount_cents.abs(), &currency, Sign::Negative)
            .trim_end_matches(currency.symbol)
            .trim_end_matches(money::NNBSP)
            .trim_start_matches(currency.symbol)
            .to_owned()
    });
    let amount = adw::EntryRow::builder()
        .title(gettext("Montant"))
        .text(&amount_text)
        .input_purpose(gtk::InputPurpose::Number)
        .build();

    // Category: "Automatique" first (proposal shown as the subtitle).
    let mut names = vec![gettext("Automatique, d’après le libellé")];
    names.extend(category_list.iter().map(|c| c.name.clone()));
    let category = adw::ComboRow::builder()
        .title(gettext("Catégorie"))
        .model(&gtk::StringList::new(
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        ))
        .build();
    if let Some(i) = initial
        .as_ref()
        .and_then(|op| op.category_id)
        .and_then(|id| category_list.iter().position(|c| c.id == id))
    {
        category.set_selected(i as u32 + 1);
    }
    let suggest = {
        let (window, category, label) = (window.downgrade(), category.clone(), label.clone());
        let category_list = category_list.clone();
        move || {
            let text = label.text();
            let proposal = (category.selected() == 0 && !text.trim().is_empty())
                .then(|| {
                    window
                        .upgrade()?
                        .read(|c| manual::suggest_category(c, &text))
                })
                .flatten()
                .and_then(|d| category_list.iter().find(|c| c.id == d.category_id))
                .map(|c| gettext("Proposée : {name}").replace("{name}", &c.name));
            category.set_subtitle(&proposal.unwrap_or_default());
        }
    };
    suggest();

    let group = adw::PreferencesGroup::new();
    for row in [
        account_row.upcast_ref::<gtk::Widget>(),
        date_row.upcast_ref(),
        label.upcast_ref(),
        direction_row.upcast_ref(),
        amount.upcast_ref(),
        category.upcast_ref(),
    ] {
        group.add(row);
    }
    let hint = gtk::Label::builder()
        .label(gettext(
            "Si l’opération arrive ensuite dans un relevé importé, Finaryx proposera de remplacer cette saisie par celle de la banque.",
        ))
        .wrap(true)
        .xalign(0.0)
        .css_classes(["caption", "dim-label"])
        .build();
    let error = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .css_classes(["error"])
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&group);
    content.append(&hint);
    content.append(&error);

    let delete = gtk::Button::builder()
        .label(gettext("Supprimer la saisie"))
        .halign(gtk::Align::Center)
        .css_classes(["destructive-action", "pill"])
        .visible(editing.is_some())
        .build();
    content.append(&delete);

    let cancel = gtk::Button::with_label(&gettext("Annuler"));
    let save = gtk::Button::builder()
        .label(if editing.is_some() {
            gettext("Enregistrer")
        } else {
            gettext("Ajouter")
        })
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&save);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&content)
        .build();
    toolbar.set_content(Some(&scrolled));
    let dialog = adw::Dialog::builder()
        .title(if editing.is_some() {
            gettext("Modifier la saisie")
        } else {
            gettext("Ajouter une opération")
        })
        .content_width(460)
        .child(&toolbar)
        .default_widget(&save)
        .build();

    // Live validation: a label and a non-zero readable amount.
    let read = {
        let (accounts, account_row, date, label, income, amount, category) = (
            accounts.clone(),
            account_row.clone(),
            date.clone(),
            label.clone(),
            income.clone(),
            amount.clone(),
            category.clone(),
        );
        let category_list = category_list.clone();
        let currency_of = currency_of.clone();
        move || -> Option<(Id, ManualOperation)> {
            let cents = money::parse_amount(&amount.text(), &currency_of())
                .ok()
                .map(i64::abs)
                .filter(|c| *c > 0);
            if cents.is_none() && !amount.text().trim().is_empty() {
                amount.add_css_class("error");
            } else {
                amount.remove_css_class("error");
            }
            let cents = cents?;
            let text = label.text().trim().to_owned();
            if text.is_empty() {
                return None;
            }
            let account = accounts.get(account_row.selected() as usize)?.id;
            Some((
                account,
                ManualOperation {
                    booked_on: date.get(),
                    label: text,
                    amount_cents: if income.is_active() { cents } else { -cents },
                    category_id: (category.selected() as usize)
                        .checked_sub(1)
                        .and_then(|i| category_list.get(i))
                        .map(|c| c.id),
                },
            ))
        }
    };
    let read = Rc::new(read);
    let update = {
        let (read, save) = (read.clone(), save.clone());
        move || save.set_sensitive(read().is_some())
    };
    update();
    for entry in [&label, &amount] {
        let update = update.clone();
        entry.connect_changed(move |_| update());
    }
    {
        let suggest = suggest.clone();
        label.connect_changed(move |_| suggest());
    }
    category.connect_selected_notify(move |_| suggest());
    {
        let update = update.clone();
        account_row.connect_selected_notify(move |_| update());
    }

    cancel.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        move |_| {
            dialog.close();
        }
    ));
    let submit = {
        let (dialog, error, window) = (dialog.clone(), error.clone(), window.downgrade());
        move || {
            let (Some((account, op)), Some(window)) = (read(), window.upgrade()) else {
                return;
            };
            let result = match txn {
                Some(id) => window.change(|conn| manual::update(conn, id, &op)),
                None => window
                    .change(|conn| manual::add(conn, account, &op))
                    .map(|_| ()),
            };
            match result {
                Ok(()) => {
                    window.toast(&if txn.is_some() {
                        gettext("Saisie modifiée")
                    } else {
                        gettext("Opération ajoutée")
                    });
                    window.signal_transfer_proposals();
                    dialog.close();
                }
                Err(message) => {
                    error.set_label(&message);
                    error.set_visible(true);
                }
            }
        }
    };
    let submit = Rc::new(submit);
    {
        let submit = submit.clone();
        save.connect_clicked(move |_| submit());
    }
    for entry in [&label, &amount] {
        let (submit, save) = (submit.clone(), save.clone());
        entry.connect_entry_activated(move |_| {
            if save.is_sensitive() {
                submit();
            }
        });
    }

    if let Some(id) = txn {
        let (dialog_ref, window_ref) = (dialog.downgrade(), window.downgrade());
        delete.connect_clicked(move |_| {
            let (Some(dialog), Some(window)) = (dialog_ref.upgrade(), window_ref.upgrade()) else {
                return;
            };
            let confirm = adw::AlertDialog::builder()
                .heading(gettext("Supprimer cette saisie ?"))
                .body(gettext("L’opération saisie à la main est effacée. Les opérations importées ne changent pas."))
                .build();
            confirm.add_responses(&[("cancel", &gettext("Annuler")), ("delete", &gettext("Supprimer"))]);
            confirm.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            confirm.set_close_response("cancel");
            let (dialog_ref, window_ref) = (dialog.downgrade(), window.downgrade());
            confirm.connect_response(None, move |_, response| {
                if response != "delete" {
                    return;
                }
                let (Some(dialog), Some(window)) = (dialog_ref.upgrade(), window_ref.upgrade())
                else {
                    return;
                };
                if window.change(|conn| manual::delete(conn, id)).is_ok() {
                    window.toast(&gettext("Saisie supprimée"));
                    dialog.close();
                }
            });
            confirm.present(Some(&dialog));
        });
    }

    dialog.present(Some(window));
    label.grab_focus();
}
