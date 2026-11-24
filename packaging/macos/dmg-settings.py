# dmgbuild settings for Waffle's disk image (release.yml): dmgbuild -s dmg-settings.py -D app=<path> -D background=<tiff> Waffle out.dmg
# Positions are the icon centres the background (dmg-background.html) leaves room for.
import os.path

app = defines["app"]  # noqa: F821, dmgbuild injects `defines`
files = [app]
symlinks = {"Applications": "/Applications"}
icon = os.path.join(app, "Contents/Resources/AppIcon.icns")
background = defines["background"]  # noqa: F821
format = "UDZO"
show_status_bar = show_tab_view = show_toolbar = show_pathbar = show_sidebar = False
# The rect is the whole window, title bar included; the content area must be the background's 660x400.
window_rect = ((200, 120), (660, 432))
default_view = "icon-view"
icon_size = 128
text_size = 13
icon_locations = {os.path.basename(app): (220, 200), "Applications": (440, 200)}
