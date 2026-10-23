import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// LP-0021 testimonial mini-app: an ordinary dApp that uses the Lezer wallet through
// LezerProvider. The wallet shows the approval sheet; this module never sees keys.
Rectangle {
    id: root
    implicitWidth: 760
    implicitHeight: 560
    color: "#0f1115"

    property string status: ""
    property string error: ""

    LezerProvider {
        id: wallet
        displayName: "Lezer Testimonials"
        onConnected: (accounts) => {
            root.error = ""
            root.status = "Connected. Choose the public account to publish from."
            authorBox.model = accounts.filter(function (a) { return a.indexOf("Public/") === 0 })
        }
        onProposalResolved: (receipt) => {
            root.error = ""
            root.status = receipt.status.state === "included"
                ? "Published on chain in block #" + receipt.status.block_id + " (tx " + receipt.tx_hash.slice(0, 16) + "…). Thank you!"
                : "Submitted (tx " + receipt.tx_hash.slice(0, 16) + "…), waiting for inclusion."
            textInput.text = ""
        }
        onFailed: (code, message) => { root.status = ""; root.error = code + ": " + message }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 20
        spacing: 12

        Label { text: "Lezer testimonials"; color: "#e8eaed"; font.pixelSize: 22; font.bold: true }
        Label {
            Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#9aa0a6"
            text: "Your testimonial is published publicly and permanently on the Logos Execution Zone, linked to the account you choose. For privacy, create a fresh public account in the Lezer wallet and share only that one."
        }

        Button {
            visible: wallet.sessionToken === ""
            text: wallet.busy ? "Waiting for approval in Lezer Wallet…" : "Connect Lezer Wallet"
            enabled: !wallet.busy
            onClicked: wallet.connect()
        }

        ColumnLayout {
            visible: wallet.sessionToken !== ""
            Layout.fillWidth: true
            spacing: 8

            Label { text: "Publish as"; color: "#9aa0a6" }
            ComboBox { id: authorBox; Layout.fillWidth: true; model: [] }
            TextField { id: usernameInput; Layout.fillWidth: true; placeholderText: "Display name (optional, public)"; maximumLength: 64 }
            TextArea {
                id: textInput
                Layout.fillWidth: true; Layout.preferredHeight: 140
                placeholderText: "What do you think of Lezer?"; wrapMode: Text.Wrap
            }
            Label { text: textInput.text.length + " / 1024"; color: textInput.text.length > 1024 ? "#f28b82" : "#9aa0a6"; font.pixelSize: 11 }
            Button {
                text: wallet.busy ? "Waiting for approval…" : "Publish testimonial"
                enabled: !wallet.busy && authorBox.currentText.length > 0 && textInput.text.trim().length > 0 && textInput.text.length <= 1024
                onClicked: {
                    root.error = ""
                    root.status = "Review and approve in Lezer Wallet."
                    wallet.propose({ kind: "submit_testimonial", author: authorBox.currentText, username: usernameInput.text.trim(), text: textInput.text.trim() })
                }
            }
        }

        Label { visible: root.status.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#81c995"; text: root.status }
        Label { visible: root.error.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#f28b82"; text: root.error }
        Item { Layout.fillHeight: true }
    }
}
