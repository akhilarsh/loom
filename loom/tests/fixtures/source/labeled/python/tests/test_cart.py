from shop.cart import Cart
from shop.pricing import total


def test_total_empty():
    assert total([]) == 0


def test_add_returns_size():
    cart = Cart()
    assert cart.add("a", 1) == 1
