from missed_call_recovery.workflow import LeadCapture, build_recovery_message, classify_lead


def sample_capture(**overrides):
    data = {
        "customer_name": "Maya",
        "phone": "530-555-0199",
        "missed_at_minutes_ago": 12,
        "repeat_calls_24h": 1,
        "service_requested": "water heater repair",
        "business_name": "Missed Call Recovery",
        "consent_to_text": True,
    }
    data.update(overrides)
    return LeadCapture(**data)


def test_baseline_message_names_customer_and_business():
    message = build_recovery_message(sample_capture())
    assert "Maya" in message
    assert "Missed Call Recovery" in message


def test_baseline_classification_is_standard():
    classification = classify_lead(sample_capture(missed_at_minutes_ago=90, repeat_calls_24h=0))
    assert classification["priority"] == "standard"
