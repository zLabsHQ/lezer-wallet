import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Onboarding & Recovery View — Create New 24-Word Vault, Restore Mnemonic, or Unlock Vault.
Item {
    id: onboardingRoot

    signal initRequested(string password)
    signal restoreRequested(string mnemonic, string password)
    signal unlockRequested(string password)

    ColumnLayout {
        anchors.centerIn: parent
        width: 480
        spacing: 12

        Label {
            text: "Lezer Wallet — Onboarding & Vault Security"
            font.pixelSize: 18
            font.bold: true
        }

        TextField {
            id: passwordField
            placeholderText: "Vault Password (Argon2id + XChaCha20-Poly1305)"
            echoMode: TextInput.Password
            Layout.fillWidth: true
        }

        TextArea {
            id: mnemonicField
            placeholderText: "Optional 24-word BIP-39 Mnemonic for Restore..."
            Layout.fillWidth: true
        }

        RowLayout {
            spacing: 8
            Button {
                text: "Create New Vault"
                onClicked: onboardingRoot.initRequested(passwordField.text)
            }
            Button {
                text: "Restore Mnemonic"
                onClicked: onboardingRoot.restoreRequested(mnemonicField.text, passwordField.text)
            }
            Button {
                text: "Unlock Existing Vault"
                onClicked: onboardingRoot.unlockRequested(passwordField.text)
            }
        }
    }
}
