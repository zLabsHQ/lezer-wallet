import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

// Create / restore / unlock. The recovery phrase is shown exactly once after creation.
Item {
    id: view
    property var store
    property string phrase: ""
    property string busyText: ""
    property string error: ""

    readonly property bool hasVault: store.lockState === "locked"

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width - 32, 560)
        spacing: 12

        Label {
            Layout.fillWidth: true
            color: "#e8eaed"; font.pixelSize: 18; wrapMode: Text.Wrap
            text: view.phrase.length ? "Write down your recovery phrase"
                 : view.hasVault ? "Unlock your wallet" : "Create or restore a wallet"
        }

        // ── One-time recovery phrase ──
        Rectangle {
            visible: view.phrase.length > 0
            Layout.fillWidth: true; radius: 8; color: "#1b1f27"; implicitHeight: phraseCol.implicitHeight + 24
            ColumnLayout {
                id: phraseCol
                anchors.fill: parent; anchors.margins: 12; spacing: 8
                Label { Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#fdd663"
                        text: "Anyone with these 24 words can take your funds. Lezer will not show them again and cannot recover them for you." }
                TextArea { Layout.fillWidth: true; readOnly: true; wrapMode: Text.Wrap; text: view.phrase; color: "#e8eaed"; font.family: "Menlo" }
                CheckBox { id: savedBox; text: "I have written down my recovery phrase" }
                Button { text: "Continue"; enabled: savedBox.checked; onClicked: { view.phrase = ""; store.refresh() } }
            }
        }

        ColumnLayout {
            visible: view.phrase.length === 0
            Layout.fillWidth: true
            spacing: 10

            TabBar {
                id: mode
                visible: !view.hasVault
                Layout.fillWidth: true
                TabButton { text: "Create new" }
                TabButton { text: "Restore" }
            }

            TextArea {
                id: phraseInput
                visible: !view.hasVault && mode.currentIndex === 1
                Layout.fillWidth: true; Layout.preferredHeight: 90
                placeholderText: "24-word recovery phrase"; wrapMode: Text.Wrap
            }

            TextField { id: pw; Layout.fillWidth: true; echoMode: TextInput.Password
                        placeholderText: view.hasVault ? "Password" : "New password (min 8 characters)"
                        onAccepted: primary.clicked() }
            TextField { id: pw2; visible: !view.hasVault; Layout.fillWidth: true; echoMode: TextInput.Password; placeholderText: "Repeat password" }

            Label { visible: view.error.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#f28b82"; text: view.error }
            Label { visible: view.busyText.length > 0; color: "#8ab4f8"; text: view.busyText }

            Button {
                id: primary
                Layout.fillWidth: true
                enabled: view.busyText.length === 0 && pw.text.length > 0
                text: view.hasVault ? "Unlock" : (mode.currentIndex === 0 ? "Create wallet" : "Restore wallet")
                onClicked: {
                    view.error = ""
                    if (view.hasVault) {
                        view.busyText = "Unlocking…"
                        store.call({ op: "unlock", password: pw.text },
                                   function () { view.busyText = ""; pw.text = ""; store.refresh() },
                                   function (e) { view.busyText = ""; view.error = Fmt.errorText(e) })
                        return
                    }
                    if (pw.text.length < 8) { view.error = "Password must be at least 8 characters."; return }
                    if (pw.text !== pw2.text) { view.error = "Passwords do not match."; return }
                    if (mode.currentIndex === 0) {
                        view.busyText = "Creating wallet…"
                        store.call({ op: "init", password: pw.text }, function (r) {
                            view.busyText = ""; pw.text = ""; pw2.text = ""
                            view.phrase = r.recovery_phrase || ""
                        }, function (e) { view.busyText = ""; view.error = Fmt.errorText(e) })
                    } else {
                        view.busyText = "Restoring and scanning the chain for your accounts…"
                        store.runJob({ op: "restore", phrase: phraseInput.text, password: pw.text, discover_depth: 3 },
                                     "restore wallet", function (st) {
                            view.busyText = ""
                            if (st.state === "failed") view.error = Fmt.errorText(st.error)
                            else { phraseInput.text = ""; pw.text = ""; pw2.text = ""; store.refresh() }
                        })
                    }
                }
            }
        }
    }
}
