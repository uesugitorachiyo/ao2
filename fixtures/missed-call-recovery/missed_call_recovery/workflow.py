from dataclasses import dataclass


@dataclass(frozen=True)
class LeadCapture:
    customer_name: str
    phone: str
    missed_at_minutes_ago: int
    repeat_calls_24h: int
    service_requested: str
    business_name: str
    consent_to_text: bool


def classify_lead(capture: LeadCapture) -> dict:
    return {
        "priority": "standard",
        "score": 50,
        "reason": "baseline missed-call follow-up",
    }


def build_recovery_message(capture: LeadCapture) -> str:
    return f"Hi {capture.customer_name}, thanks for calling {capture.business_name}."
