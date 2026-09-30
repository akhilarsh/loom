import sys

from shop.cart import Cart


def main(argv):
    cart = Cart()
    for name in argv:
        cart.add(name, 10)
    return cart.checkout("http://localhost/pay")


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
