use futures::prelude::*;
use gtk4::{glib, prelude::*};
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent, WidgetPlus};
use std::{process::Command, time::Duration};

mod backlight;
mod upower;
use upower::UPowerProxy;
mod upower_device;
use upower_device::DeviceProxy;

async fn display_device() -> zbus::Result<DeviceProxy<'static>> {
    let connection = zbus::Connection::system().await?;
    let upower = UPowerProxy::new(&connection).await?;
    let device_path = upower.get_display_device().await?;
    DeviceProxy::builder(&connection)
        .path(device_path)?
        .cache_properties(zbus::CacheProperties::Yes)
        .build()
        .await
}

#[derive(Default)]
struct AppModel {
    icon_name: String,
    battery_percent: f64,
    time_remaining: Duration,
    display_brightness: f64,
    keyboard_brightness: f64,
    device: Option<DeviceProxy<'static>>,
}

enum AppMsg {
    SetDisplayBrightness(f64),
    SetKeyboardBrightness(f64),
    SetDevice(DeviceProxy<'static>),
    UpdateProperties,
}

#[relm4::component]
impl SimpleComponent for AppModel {
    type Widgets = AppWidgets;

    type InitParams = ();

    type Input = AppMsg;
    type Output = ();

    view! {
        gtk4::Window {
            gtk4::MenuButton {
                set_has_frame: false,
                #[watch]
                set_icon_name: &model.icon_name,
                #[wrap(Some)]
                set_popover = &gtk4::Popover {
                    #[wrap(Some)]
                    set_child = &gtk4::Box {
                        set_orientation: gtk4::Orientation::Vertical,

                        // Battery
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Horizontal,
                            gtk4::Image {
                                #[watch]
                                set_icon_name: Some(&model.icon_name),
                            },
                            gtk4::Box {
                                set_orientation: gtk4::Orientation::Vertical,
                                gtk4::Label {
                                    set_halign: gtk4::Align::Start,
                                    set_label: "Battery",
                                },
                                gtk4::Label {
                                    set_halign: gtk4::Align::Start,
                                    // XXX duration formatting
                                    // XXX time to full, fully changed, etc.
                                    #[watch]
                                    set_label: &format!("{:?} until empty ({:.0}%)", model.time_remaining, model.battery_percent),
                                },
                            },
                        },

                        gtk4::Separator {
                        },

                        // Profiles

                        gtk4::Separator {
                        },

                        // Limit charging
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Horizontal,
                            gtk4::Box {
                                set_orientation: gtk4::Orientation::Vertical,
                                gtk4::Label {
                                    set_halign: gtk4::Align::Start,
                                    set_label: "Limit Battery Charging",
                                },
                                gtk4::Label {
                                    set_halign: gtk4::Align::Start,
                                    set_label: "Increase the lifespan of your battery by setting a maximum charge value of 80%."
                                },
                            },
                            gtk4::Switch {
                                set_valign: gtk4::Align::Center,
                            },
                        },

                        gtk4::Separator {
                        },

                        // Brightness
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Horizontal,
                            gtk4::Image {
                                set_icon_name: Some("display-brightness-symbolic"),
                            },
                            gtk4::Scale {
                                set_hexpand: true,
                                set_adjustment: &gtk4::Adjustment::new(0., 0., 100., 1., 1., 0.),
                                #[watch]
                                set_value: model.display_brightness,
                                connect_change_value[sender] => move |_, _, value| {
                                    sender.input(AppMsg::SetDisplayBrightness(value));
                                    gtk4::Inhibit(false)
                                },
                            },
                            gtk4::Label {
                                #[watch]
                                set_label: &format!("{:.0}%", model.display_brightness),
                            },
                        },
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Horizontal,
                            gtk4::Image {
                                set_icon_name: Some("keyboard-brightness-symbolic"),
                            },
                            gtk4::Scale {
                                set_hexpand: true,
                                set_adjustment: &gtk4::Adjustment::new(0., 0., 100., 1., 1., 0.),
                                #[watch]
                                set_value: model.keyboard_brightness,
                                connect_change_value[sender] => move |_, _, value| {
                                    sender.input(AppMsg::SetKeyboardBrightness(value));
                                    gtk4::Inhibit(false)
                                },
                            },
                            gtk4::Label {
                                #[watch]
                                set_label: &format!("{:.0}%", model.keyboard_brightness),
                            },
                        },

                        gtk4::Separator {
                        },

                        gtk4::Button {
                            set_label: "Power Settings...",
                            connect_clicked => move |_| {
                                // XXX open subpanel
                                let _ = Command::new("cosmic-settings").spawn();
                                // TODO hide
                            }
                        }
                    }
                }
            }
        }
    }

    fn init(
        _params: Self::InitParams,
        root: &Self::Root,
        sender: &ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = AppModel {
            icon_name: "battery-symbolic".to_string(),
            ..Default::default()
        };

        let widgets = view_output!();

        let sender = sender.clone();
        glib::MainContext::default().spawn(async move {
            match display_device().await {
                Ok(device) => sender.input(AppMsg::SetDevice(device)),
                Err(err) => eprintln!("Failed to open UPower display device: {}", err),
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: &ComponentSender<Self>) {
        match msg {
            AppMsg::SetDisplayBrightness(value) => {
                self.display_brightness = value;
                // XXX set brightness
            }
            AppMsg::SetKeyboardBrightness(value) => {
                self.keyboard_brightness = value;
                // XXX set brightness
            }
            AppMsg::SetDevice(device) => {
                self.device = Some(device.clone());

                let sender = sender.clone();
                glib::MainContext::default().spawn(async move {
                    let mut stream = futures::stream_select!(
                        device.receive_icon_name_changed().await.map(|_| ()),
                        device.receive_percentage_changed().await.map(|_| ()),
                        device.receive_time_to_empty_changed().await.map(|_| ()),
                    );

                    sender.input(AppMsg::UpdateProperties);
                    while let Some(()) = stream.next().await {
                        sender.input(AppMsg::UpdateProperties);
                    }
                });
            }
            AppMsg::UpdateProperties => {
                if let Some(device) = self.device.as_ref() {
                    if let Ok(Some(percentage)) = device.cached_percentage() {
                        self.battery_percent = percentage;
                    }
                    if let Ok(Some(icon_name)) = device.cached_icon_name() {
                        self.icon_name = icon_name;
                    }
                    if let Ok(Some(secs)) = device.cached_time_to_empty() {
                        self.time_remaining = Duration::from_secs(secs as u64);
                    }
                }
            }
        }
    }
}

fn main() {
    let app: RelmApp<AppModel> = RelmApp::new("com.system76.CosmicAppletBattery");
    app.run(());
}