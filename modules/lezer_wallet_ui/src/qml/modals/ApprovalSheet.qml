import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

// Renders ONLY the TransactionSummary built in Rust and returns a decision bound to its
// digest. The UI cannot change what is signed. Unverified/critical gates require explicit
// acknowledgement; a failed balance precheck blocks approval.
Rectangle {
    id: sheet
    property var request
    property var store
    property string error: ""
    property bool submitting: false
    color: "#cc000000"

    readonly property var s: request ? request.summary_json : null
    readonly property bool blocked: !!(s && s.precheck && (s.precheck.result === "insufficient_funds" || s.precheck.result === "blocked"))
    readonly property var badgeColors: ({
        protocol_builtin: "#81c995", verified_source: "#81c995", known_but_unverified: "#8ab4f8",
        no_registry_entry: "#fdd663", mismatch_or_error: "#f28b82"
    })

    onRequestChanged: { error = ""; submitting = false; ackBox.checked = false; overrideBox.checked = false }
    MouseArea { anchors.fill: parent }

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(parent.width - 48, 620); radius: 12; color: "#1b1f27"
        implicitHeight: col.implicitHeight + 32
        height: implicitHeight
        ColumnLayout {
            id: col
            anchors.fill: parent; anchors.margins: 16; spacing: 10
            visible: !!sheet.s

            Label { text: "Approve transaction"; color: "#e8eaed"; font.pixelSize: 18; font.bold: true }
            Label { text: "Requested by module “" + (sheet.s ? sheet.s.originating_dapp : "") + "”"; color: "#9aa0a6" }
            Label { Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#e8eaed"; font.pixelSize: 16; text: sheet.s ? sheet.s.headline : "" }

            GridLayout {
                columns: 2; columnSpacing: 12; rowSpacing: 4
                Label { text: "Amount"; color: "#9aa0a6" }      Label { text: sheet.s ? Fmt.toDisplay(sheet.s.amount) : ""; color: "#e8eaed" }
                Label { text: "From"; color: "#9aa0a6" }        Label { text: sheet.s ? sheet.s.from : ""; color: "#e8eaed"; elide: Text.ElideMiddle; Layout.maximumWidth: 440 }
                Label { text: "To"; color: "#9aa0a6" }          Label { text: sheet.s ? sheet.s.to : ""; color: "#e8eaed"; elide: Text.ElideMiddle; Layout.maximumWidth: 440 }
                Label { text: "Privacy"; color: "#9aa0a6" }     Label { text: sheet.s ? sheet.s.privacy_mode : ""; color: "#e8eaed" }
                Label { text: "Max fee"; color: "#9aa0a6"; visible: !!(sheet.s && sheet.s.max_fee) }
                Label { text: sheet.s && sheet.s.max_fee ? Fmt.toDisplay(sheet.s.max_fee) : ""; color: "#e8eaed"; visible: !!(sheet.s && sheet.s.max_fee) }
                Label { text: "Program"; color: "#9aa0a6" }
                Label {
                    text: sheet.s ? sheet.s.verification_badge.label : ""
                    color: sheet.s ? (sheet.badgeColors[sheet.s.verification_badge.state] || "#fdd663") : "#fdd663"
                }
            }

            Repeater {
                model: sheet.s ? sheet.s.warnings : []
                Label {
                    Layout.fillWidth: true; wrapMode: Text.Wrap
                    color: modelData.severity === "critical_red_override" ? "#f28b82" : modelData.severity === "amber_gate" ? "#fdd663" : "#9aa0a6"
                    text: "• " + modelData.title + ": " + modelData.message
                }
            }

            Label {
                visible: sheet.blocked
                Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#f28b82"
                text: !sheet.blocked ? ""
                      : sheet.s.precheck.result === "blocked" ? sheet.s.precheck.reason
                      : "Insufficient balance: available " + Fmt.toDisplay(sheet.s.precheck.available) + ", required " + Fmt.toDisplay(sheet.s.precheck.required)
            }
            CheckBox { id: ackBox; visible: !!(sheet.s && sheet.s.requires_unverified_ack); text: "I understand this program's source is not verified" }
            CheckBox { id: overrideBox; visible: !!(sheet.s && sheet.s.requires_critical_override); text: "I understand this is a critical, irreversible operation" }
            Label { visible: sheet.error.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#f28b82"; text: sheet.error }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                Button {
                    text: "Reject"
                    enabled: !sheet.submitting
                    onClicked: store.call({ op: "reject", request_id: sheet.request.request_id, reason: "Rejected by user" }, function () { store.refresh() })
                }
                Button {
                    text: sheet.submitting ? "Executing…" : "Approve"
                    highlighted: true
                    enabled: !!sheet.s && !sheet.submitting && !sheet.blocked
                             && (!sheet.s.requires_unverified_ack || ackBox.checked)
                             && (!sheet.s.requires_critical_override || overrideBox.checked)
                    onClicked: {
                        sheet.submitting = true
                        store.runJob({ op: "approve_proposal", decision: {
                                proposal_id: sheet.request.request_id,
                                summary_digest_hex: sheet.s.summary_digest_hex,
                                acknowledged_unverified: ackBox.checked,
                                critical_override: overrideBox.checked } },
                            "approve", function (st) {
                                sheet.submitting = false
                                if (st.state === "failed") sheet.error = Fmt.errorText(st.error)
                                store.refresh()
                            })
                    }
                }
            }
        }
    }
}
