//! Dialog to add an account or change its settings.

use adw::prelude::*;
use finaryx_core::accounts::AccountChanges;
use finaryx_core::models::AccountKind;
use finaryx_core::money::{self, Currency, Sign};
use gettextrs::gettext;
use gtk::glib;

/// Which field gets the focus when the dialog opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Bank,
    Kind,
    Currency,
    OpeningBalance,
}

/// Opens the dialog. `initial` is `None` to add an account. `on_save`
/// receives the validated changes and returns an error message to show,
/// or `None` to close the dialog.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    initial: Option<&AccountChanges>,
    focus: Field,
    on_save: impl Fn(AccountChanges) -> Option<String> + 'static,
) {
    let adding = initial.is_none();
    let default = AccountChanges {
        name: String::new(),
        bank_name: String::new(),
        number: None,
        kind: AccountKind::Checking,
        currency: "EUR".into(),
        opening_balance_cents: 0,
    };
    let initial = initial.cloned().unwrap_or(default);

    let name = adw::EntryRow::builder()
        .title(gettext("Nom"))
        .text(&initial.name)
        .build();
    let bank = adw::EntryRow::builder()
        .title(gettext("Banque"))
        .text(&initial.bank_name)
        .build();
    let kind = adw::ComboRow::builder()
        .title(gettext("Type"))
        .model(&gtk::StringList::new(&[
            &gettext("Compte courant"),
            &gettext("Épargne"),
        ]))
        .selected(if initial.kind == AccountKind::Savings {
            1
        } else {
            0
        })
        .build();

    let currencies: Vec<Currency> = Currency::known().to_vec();
    let currency_labels: Vec<String> = currencies
        .iter()
        .map(|c| format!("{} ({})", c.code, c.symbol))
        .collect();
    let currency_labels: Vec<&str> = currency_labels.iter().map(String::as_str).collect();
    let currency = adw::ComboRow::builder()
        .title(gettext("Devise"))
        .subtitle(gettext(
            "Les totaux sont calculés séparément pour chaque devise.",
        ))
        .model(&gtk::StringList::new(&currency_labels))
        .selected(
            currencies
                .iter()
                .position(|c| c.code == initial.currency)
                .unwrap_or(0) as u32,
        )
        .build();

    let number = adw::EntryRow::builder()
        .title(gettext("Numéro de compte (facultatif)"))
        .text(initial.number.as_deref().unwrap_or(""))
        .build();

    let initial_currency = Currency::from_code(&initial.currency).unwrap_or(Currency::EUR);
    let opening_text = if adding && initial.opening_balance_cents == 0 {
        String::new()
    } else {
        // Without the symbol, so that the text can be edited and re-read.
        let text = money::format_amount(
            initial.opening_balance_cents,
            &initial_currency,
            Sign::Negative,
        );
        text.trim_end_matches(initial_currency.symbol)
            .trim_end_matches(money::NNBSP)
            .to_owned()
    };
    let opening = adw::EntryRow::builder()
        .title(gettext("Solde avant la première opération importée"))
        .text(&opening_text)
        .input_purpose(gtk::InputPurpose::Number)
        .build();

    let group = adw::PreferencesGroup::new();
    for row in [
        name.upcast_ref::<gtk::Widget>(),
        bank.upcast_ref(),
        kind.upcast_ref(),
        currency.upcast_ref(),
        number.upcast_ref(),
        opening.upcast_ref(),
    ] {
        group.add(row);
    }
    let hint = gtk::Label::builder()
        .label(gettext(
            "Laissez le solde vide s’il est nul. Si le fichier importé contient une colonne de solde, \
             Finaryx proposera de le recaler.",
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

    let cancel = gtk::Button::with_label(&gettext("Annuler"));
    let save = gtk::Button::builder()
        .label(if adding {
            gettext("Ajouter")
        } else {
            gettext("Enregistrer")
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
    toolbar.set_content(Some(&content));

    let dialog = adw::Dialog::builder()
        .title(if adding {
            gettext("Ajouter un compte")
        } else {
            gettext("Modifier le compte")
        })
        .content_width(460)
        .child(&toolbar)
        .default_widget(&save)
        .build();

    // Live validation: a name, and a readable amount.
    let read = {
        let (name, bank, kind, currency, number, opening) = (
            name.clone(),
            bank.clone(),
            kind.clone(),
            currency.clone(),
            number.clone(),
            opening.clone(),
        );
        let currencies = currencies.clone();
        move || -> Option<AccountChanges> {
            let cur = currencies
                .get(currency.selected() as usize)
                .copied()
                .unwrap_or(Currency::EUR);
            let text = opening.text();
            let amount = if text.trim().is_empty() {
                Ok(0)
            } else {
                money::parse_amount(&text, &cur)
            };
            if amount.is_err() {
                opening.add_css_class("error");
            } else {
                opening.remove_css_class("error");
            }
            let changes = AccountChanges {
                name: name.text().trim().to_owned(),
                bank_name: bank.text().trim().to_owned(),
                number: Some(number.text().trim().to_owned()).filter(|n| !n.is_empty()),
                kind: if kind.selected() == 1 {
                    AccountKind::Savings
                } else {
                    AccountKind::Checking
                },
                currency: cur.code.to_owned(),
                opening_balance_cents: amount.ok()?,
            };
            (!changes.name.is_empty()).then_some(changes)
        }
    };
    let read = std::rc::Rc::new(read);
    let update = {
        let (read, save) = (read.clone(), save.clone());
        move || save.set_sensitive(read().is_some())
    };
    update();
    for entry in [&name, &opening] {
        let update = update.clone();
        entry.connect_changed(move |_| update());
    }
    {
        let update = update.clone();
        currency.connect_selected_notify(move |_| update());
    }

    cancel.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        move |_| {
            dialog.close();
        }
    ));
    let submit = {
        let (dialog, error) = (dialog.clone(), error.clone());
        move || {
            if let Some(changes) = read() {
                match on_save(changes) {
                    None => {
                        dialog.close();
                    }
                    Some(message) => {
                        error.set_label(&message);
                        error.set_visible(true);
                    }
                }
            }
        }
    };
    let submit = std::rc::Rc::new(submit);
    {
        let submit = submit.clone();
        save.connect_clicked(move |_| submit());
    }
    for entry in [&name, &bank, &number, &opening] {
        let (submit, save) = (submit.clone(), save.clone());
        entry.connect_entry_activated(move |_| {
            if save.is_sensitive() {
                submit();
            }
        });
    }

    dialog.present(Some(parent));
    let target: gtk::Widget = match focus {
        Field::Name => name.upcast(),
        Field::Bank => bank.upcast(),
        Field::Kind => kind.upcast(),
        Field::Currency => currency.upcast(),
        Field::OpeningBalance => opening.upcast(),
    };
    target.grab_focus();
}
