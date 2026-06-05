from discount_service.discounts import calculate_discount


def test_calculates_discount_for_valid_values():
    assert calculate_discount(100, 0.25) == 75

