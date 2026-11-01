pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

// Fungible tokens via the deployed lezer_token program. Balances are exact on-chain reads;
// the tracked-token list only remembers which token ids this wallet has touched.
Item {
    id: view
    property var store

    property var tokens: ({})          // token_id -> TokenInfo
    property var tokenIds: []
    property var tokenBalances: ({})   // token_id + "|" + account_id -> balance string
    property string tokenProgram: ""

    function refresh() {
        view.store.call({ op: "tokens" }, function (r) {
            view.tokenProgram = r.token_program || ""
            view.tokenIds = r.token_ids || []
            view.tokenIds.forEach(function (id) {
                view.store.call({ op: "token_info", token_id: id }, function (info) {
                    var m = Object.assign({}, view.tokens); m[id] = info; view.tokens = m
                }, function () {})
            })
            view.refreshBalances()
        }, function () {})
    }

    function refreshBalances() {
        view.tokenIds.forEach(function (tid) {
            view.store.accounts.forEach(function (a) {
                if (a.kind !== "public") return
                view.store.call({ op: "token_balance", token_id: tid, account: a.account_id }, function (r) {
                    var m = Object.assign({}, view.tokenBalances); m[tid + "|" + a.account_id] = r.balance; view.tokenBalances = m
                }, function () {})
            })
        })
    }

    function balanceOf(tid, accountId) { return view.tokenBalances[tid + "|" + accountId] || "0" }

    property string accountKey: view.store.accounts.map(function (a) { return a.account_id }).join(",")
    onAccountKeyChanged: refreshBalances()
    Component.onCompleted: refresh()
    Connections { target: view.store; function onJobFinished() { view.refresh() } }

    ColumnLayout {
        anchors.fill: parent; spacing: 10

        RowLayout {
            Layout.fillWidth: true
            Label { text: "Tokens" + (view.tokenProgram ? "" : "  (no lezer_token deployed — run `lezer program deploy lezer_token.bin`)"); color: "#9aa0a6"; Layout.fillWidth: true; elide: Text.ElideRight }
            TextField {
                id: trackId; placeholderText: "Track token id (Public/…)"; Layout.preferredWidth: 260
            }
            Button {
                text: "Track"
                enabled: trackId.text.length > 0
                onClicked: view.store.call({ op: "token_track", token_id: trackId.text }, function () { trackId.text = ""; view.refresh() })
            }
        }

        ListView {
            Layout.fillWidth: true; Layout.fillHeight: true; clip: true; spacing: 8
            model: view.tokenIds
            delegate: Rectangle {
                id: tokenRow
                required property string modelData
                readonly property string tokenId: modelData
                readonly property var info: view.tokens[tokenId]
                width: ListView.view.width; height: tokenCol.implicitHeight + 24; radius: 8; color: "#1b1f27"
                ColumnLayout {
                    id: tokenCol
                    anchors.fill: parent; anchors.margins: 12; spacing: 6
                    RowLayout {
                        Layout.fillWidth: true
                        Label { text: tokenRow.info ? tokenRow.info.symbol : "…"; color: "#e8eaed"; font.bold: true; font.pixelSize: 15 }
                        Label { text: tokenRow.info ? tokenRow.info.name : ""; color: "#9aa0a6" }
                        Item { Layout.fillWidth: true }
                        Label { text: tokenRow.info ? "supply " + tokenRow.info.total_supply : ""; color: "#9aa0a6"; font.pixelSize: 11 }
                        Label { text: tokenRow.info && tokenRow.info.authority ? "authority " + Fmt.shortId(tokenRow.info.authority) : "fixed supply"; color: "#9aa0a6"; font.pixelSize: 11 }
                    }
                    Label { text: tokenRow.tokenId; color: "#6b7280"; font.pixelSize: 10; elide: Text.ElideMiddle; Layout.fillWidth: true }

                    Repeater {
                        model: view.store.accounts.filter(function (a) { return a.kind === "public" })
                        delegate: RowLayout {
                            id: holderRow
                            required property var modelData
                            Layout.fillWidth: true
                            Label { text: Fmt.label(holderRow.modelData); color: "#c5c8ce"; font.pixelSize: 12; Layout.preferredWidth: 140 }
                            Label {
                                color: "#e8eaed"; Layout.fillWidth: true
                                text: Fmt.toDisplayD(view.balanceOf(tokenRow.tokenId, holderRow.modelData.account_id),
                                                     tokenRow.info ? tokenRow.info.decimals : 9)
                            }
                            Button {
                                text: "Send"; enabled: tokenRow.info !== undefined
                                onClicked: sendSheet.openFor(tokenRow.tokenId, holderRow.modelData.account_id)
                            }
                        }
                    }
                }
            }
        }
    }

    // Send-token sheet — stays inside the wallet UI; uses exact atomic amounts.
    Dialog {
        id: sendSheet
        property string tokenId: ""
        property string fromAccount: ""
        anchors.centerIn: parent; width: 420; modal: true; title: "Send tokens"
        function openFor(tid, from) { tokenId = tid; fromAccount = from; open() }
        ColumnLayout {
            width: parent.width; spacing: 8
            Label { text: "From " + Fmt.shortId(sendSheet.fromAccount); color: "#9aa0a6"; elide: Text.ElideMiddle }
            ComboBox {
                id: toBox; Layout.fillWidth: true
                model: view.store.accounts.filter(function (a) { return a.kind === "public" && a.account_id !== sendSheet.fromAccount })
                textRole: "account_id"; displayText: currentText ? Fmt.shortId(currentText) : ""
            }
            TextField { id: amountField; placeholderText: "Amount (token units)"; Layout.fillWidth: true }
            Label { id: sendErr; color: "#f28b82"; wrapMode: Text.Wrap; Layout.fillWidth: true }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                Button { text: "Cancel"; onClicked: sendSheet.close() }
                Button {
                    text: "Send"; enabled: amountField.text.length > 0 && toBox.currentIndex >= 0
                    onClicked: {
                        var info = view.tokens[sendSheet.tokenId]
                        var atomic = Fmt.toAtomicD(amountField.text, info ? info.decimals : 9)
                        if (!atomic) { sendErr.text = "invalid amount"; return }
                        sendErr.text = ""
                        sendSheet.close()
                        view.store.runJob({ op: "token_transfer", token_id: sendSheet.tokenId,
                                       from: sendSheet.fromAccount, to: toBox.currentText,
                                       amount: atomic, payer: sendSheet.fromAccount }, "token transfer", function () { view.refresh() })
                    }
                }
            }
        }
    }
}
