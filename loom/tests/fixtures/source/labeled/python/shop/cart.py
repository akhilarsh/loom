import requests

from shop import compute_total
from .models import Item as Product


class Cart:
    def __init__(self):
        self.products = []

    def add(self, name, price):
        self.products.append(Product(name, price))
        return self.size()

    def size(self):
        return len(self.products)

    def checkout(self, url):
        amount = compute_total(self.products)
        requests.post(url, json={"amount": amount})
        return amount
