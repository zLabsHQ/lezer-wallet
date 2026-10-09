import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Approval Sheet — Renders canonical `TransactionSummary` from Rust, 4-state LP-0023
// `SourceVerificationBadge`, `PUBLIC ON-CHAIN PRIVACY NOTICE` banner, and returns
// `ApproveDecision { proposal_id, summary_digest_hex, acknowledged_unverified, critical_override }`.
Item {
    id: approvalSheet

    property string proposalId: ""
    property string originatingDapp: ""
    property string signingAccount: ""
    property string headline: ""
    property string verificationBadgeLabel: "✓ DEPLOYER VERIFIED & LOCAL REBUILD MATCH"
    property string summaryDigestHex: ""
    property bool requiresUnverifiedAck: false
    property bool requiresCriticalOverride: false
    property bool showPublicPrivacyNotice: true
    property int estimatedCycles: 155648

    signal approveProposal(string proposalId, string summaryDigestHex, bool ackUnverified, bool criticalOverride)
    signal rejectProposal(string proposalId)

    ColumnLayout {
        anchors.fill: parent
        spacing: 10

        Label {
            text: "LEZER WALLET — APPROVE TRANSACTION"
            font.bold: true
        }

        Label {
            text: "Originating App: " + approvalSheet.originatingDapp + " | Signer: " + approvalSheet.signingAccount
        }

        Rectangle {
            Layout.fillWidth: true
            height: 44
            color: "#14291d"
            Label {
                anchors.centerIn: parent
                text: "LP-0023 Badge: " + approvalSheet.verificationBadgeLabel
            }
        }

        Label {
            text: "Decoded Effect: " + approvalSheet.headline
            font.bold: true
        }

        Rectangle {
            visible: approvalSheet.showPublicPrivacyNotice
            Layout.fillWidth: true
            height: 52
            color: "#1e2638"
            Label {
                anchors.centerIn: parent
                text: "PUBLIC ON-CHAIN PRIVACY NOTICE: Testimonials are permanent public records tied to your signing account. Do not include sensitive personal data."
            }
        }

        Label {
            text: "Estimated RISC0 Cycles: ~" + approvalSheet.estimatedCycles + " cycles"
        }

        CheckBox {
            id: unverifiedAckBox
            visible: approvalSheet.requiresUnverifiedAck
            text: "I understand this program has no LP-0023 registry entry / unverified source"
        }

        CheckBox {
            id: criticalOverrideBox
            visible: approvalSheet.requiresCriticalOverride
            text: "CRITICAL RED OVERRIDE: I confirm this permanent authority mutation"
        }

        RowLayout {
            spacing: 8
            Button {
                text: "Reject Transaction"
                onClicked: approvalSheet.rejectProposal(approvalSheet.proposalId)
            }
            Button {
                text: "Sign & Broadcast Tx"
                enabled: (!approvalSheet.requiresUnverifiedAck || unverifiedAckBox.checked)
                         && (!approvalSheet.requiresCriticalOverride || criticalOverrideBox.checked)
                onClicked: approvalSheet.approveProposal(
                    approvalSheet.proposalId,
                    approvalSheet.summaryDigestHex,
                    unverifiedAckBox.checked,
                    criticalOverrideBox.checked
                )
            }
        }
    }
}
