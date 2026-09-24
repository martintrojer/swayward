---
name: Bug report
about: Report a bug or a crash
title: ''
type: Bug
assignees: ''

---

<!-- Please describe the issue here at the top, then fill in the system information below. -->

<!-- Attaching your full swayward config can help diagnose the problem. -->
<details><summary>Config</summary>

```kdl
insert config here
```

</details> 

<!--
If you have a problem with a specific app, please verify that it is running on Wayland, rather than X11. An easy way is to run xeyes and mouse over the app: xeyes will be able to "see" only X11 windows.

You can also inspect the focused window through sway IPC:

$ swaywardmsg -t get_tree | jq '.. | objects | select(.focused? == true and .pid? != null) | {app_id, name, pid, shell}'

A `shell` value of `xwayland` identifies an X11 window.

Please report issues with X11 apps to xwayland-satellite instead of swayward: https://github.com/Supreeeme/xwayland-satellite/issues
-->

### System Information

<!-- Paste the output of `swayward -V`. -->
* swayward version:

<!-- Write your distribution, e.g. Fedora 40 Silverblue -->
* Distro: 

<!-- Write your GPU vendor and model, e.g. AMD RX 6700M -->
* GPU: 

<!-- Write your CPU vendor and model, e.g. AMD Ryzen 7 6800H -->
* CPU:
