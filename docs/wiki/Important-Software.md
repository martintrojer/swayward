Since swayward is not a complete desktop environment, you will very likely want to run the following software to make sure that other apps work fine.

### Notification Daemon

Many apps need one. For example, [mako](https://github.com/emersion/mako) works well. Use [a systemd setup](./Example-systemd-Setup.md) or [`spawn-at-startup`](./Configuration:-Miscellaneous.md#spawn-at-startup).

### Portals

These provide a cross-desktop API for apps to use for various things like file pickers or UI settings. Flatpak apps in particular require working portals.

Portals **require** [running swayward as a session](./Getting-Started.md), which means through the `swayward-session` script or from a display manager. You will want the following portals installed:

* `xdg-desktop-portal-gnome`: the preferred backend for the integrated window and monitor picker, PipeWire streams, and dynamic cast target.
* `xdg-desktop-portal-gtk`: the fallback backend and the provider for Access and Notification.
* `nautilus`: provides the FileChooser implementation used by `xdg-desktop-portal-gnome` 47 and later.
* `gnome-keyring`: implements the Secret portal, required for certain apps to work.

Then systemd should start them on-demand automatically. These particular portals are configured in `swayward-portals.conf` which [must be installed](./Getting-Started.md#manual-installation) in the correct location.

Since we're using `xdg-desktop-portal-gnome`, Flatpak apps will read the GNOME UI settings. For example, to enable the dark style, run:

```
dconf write /org/gnome/desktop/interface/color-scheme '"prefer-dark"'
```

With `resources/swayward-portals.conf`, install Nautilus if you use `xdg-desktop-portal-gnome` 47 or later. GNOME 47 moved its FileChooser implementation into Nautilus, so the dialog does not open when the GNOME backend is selected and Nautilus is absent.

To avoid installing Nautilus, set `org.freedesktop.impl.portal.FileChooser=gtk;` in `swayward-portals.conf`. This keeps the GNOME ScreenCast and Screenshot interfaces while routing only file chooser dialogs to the GTK backend.

As a less integrated capture fallback, install `xdg-desktop-portal-wlr` and set both `org.freedesktop.impl.portal.ScreenCast=wlr;` and `org.freedesktop.impl.portal.Screenshot=wlr;`. The wlr backend does not provide swayward's GNOME window picker or dynamic cast target.

> [!WARNING]
> Do not set the `GDK_BACKEND` environment variable globally as this will break the screencast portal.

### Authentication Agent

Required when apps need to ask for root permissions. Something like `plasma-polkit-agent` works fine. Start it [with systemd](./Example-systemd-Setup.md) or with [`spawn-at-startup`](./Configuration:-Miscellaneous.md#spawn-at-startup).

Note that to start `plasma-polkit-agent` with systemd on Fedora, you'll need to override its systemd service to add the correct dependency. Run:

```
systemctl --user edit --full plasma-polkit-agent.service
```

Then add `After=graphical-session.target`.

### Xwayland

To run X11 apps like Steam or Discord, you can use [xwayland-satellite].
Check [the Xwayland wiki page](./Xwayland.md) for instructions.

[xwayland-satellite]: https://github.com/Supreeeme/xwayland-satellite

---

*This page is adapted from the niri documentation.*
