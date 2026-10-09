import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Token & ATA Manager View — Create fungible token, derive/initialize ATAs, and LP-0013 Mint Authority controls.
Item {
    id: tokenRoot

    signal createTokenRequested(string symbol, string initialSupply, string authority)
    signal setAuthorityRequested(string definitionId, string newAuthority)

    ColumnLayout {
        anchors.fill: parent
        spacing: 10

        Label {
            text: "Fungible Token Program (TOKEN_ID) & Associated Token Accounts (ATAs)"
            font.bold: true
        }

        TextField {
            id: symbolField
            placeholderText: "Token Symbol (e.g. LEZERUSD)"
            Layout.fillWidth: true
        }

        TextField {
            id: supplyField
            placeholderText: "Initial Supply (e.g. 1000000)"
            Layout.fillWidth: true
        }

        TextField {
            id: authorityField
            placeholderText: "LP-0013 Mint Authority Account (or 'none' for fixed supply)"
            Layout.fillWidth: true
        }

        RowLayout {
            spacing: 8
            Button {
                text: "Create Fungible Token + ATA"
                onClicked: tokenRoot.createTokenRequested(symbolField.text, supplyField.text, authorityField.text)
            }
            Button {
                text: "Rotate / Revoke Mint Authority (LP-0013)"
                onClicked: tokenRoot.setAuthorityRequested(symbolField.text, authorityField.text)
            }
        }
    }
}
