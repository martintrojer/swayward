import QtQuick
import Quickshell
import Quickshell.I3

ShellRoot {
    FloatingWindow {
        implicitWidth: 1
        implicitHeight: 1
        visible: true
    }

    property bool dispatched: false
    property bool sawEvent: false
    property int attempts: 0

    I3IpcListener {
        subscriptions: ["workspace"]
        onIpcEvent: event => {
            if (event.type === "workspace") {
                sawEvent = true
                console.info("PROBE workspace-event type=" + event.type)
            }
        }
    }

    Timer {
        interval: 100
        repeat: true
        running: true
        onTriggered: {
            attempts++
            const workspaces = I3.workspaces.values
            const monitors = I3.monitors.values

            if (workspaces.length > 0 && monitors.length > 0 && !dispatched) {
                const workspace = workspaces[0]
                const monitor = monitors[0]
                if (workspace.id === undefined || workspace.name === undefined ||
                    workspace.number === undefined || workspace.focused === undefined ||
                    workspace.urgent === undefined || workspace.monitor === undefined) {
                    console.warn("PROBE FAIL: workspace model is missing a required field")
                    Qt.quit()
                    return
                }
                if (monitor.x === undefined || monitor.y === undefined ||
                    monitor.width === undefined || monitor.height === undefined ||
                    monitor.scale === undefined || monitor.width <= 0 ||
                    monitor.height <= 0 || monitor.scale <= 0) {
                    console.warn("PROBE FAIL: monitor geometry or scale is missing or invalid")
                    Qt.quit()
                    return
                }

                console.info("PROBE workspace id=" + workspace.id + " name=" + workspace.name +
                            " number=" + workspace.number + " focused=" + workspace.focused +
                            " urgent=" + workspace.urgent + " monitor=" + workspace.monitor.name)
                console.info("PROBE monitor name=" + monitor.name + " geometry=" + monitor.x + "," +
                            monitor.y + " " + monitor.width + "x" + monitor.height +
                            " scale=" + monitor.scale)
                dispatched = true
                I3.dispatch("workspace number 2")
                return
            }

            if (dispatched && sawEvent && I3.focusedWorkspace && I3.focusedWorkspace.number === 2) {
                console.info("PROBE PASS focused-workspace=2")
                Qt.quit()
            } else if (attempts >= 100) {
                console.warn("PROBE FAIL: timed out waiting for Quickshell.I3 state or workspace event")
                Qt.quit()
            }
        }
    }
}
