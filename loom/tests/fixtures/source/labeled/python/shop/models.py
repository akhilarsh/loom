class Item:
    def __init__(self, name, price, qty=1):
        self.name = name
        self._price = price
        self.qty = qty

    @property
    def price(self):
        return self._price

    @price.setter
    def price(self, value):
        self._price = self._check(value)

    def _check(self, value):
        return max(value, 0)

    def describe(self):
        return f"{self.name} x{self.qty}"


class Bundle:
    def __init__(self, items):
        self.items = items

    def describe(self):
        return ", ".join(i.describe() for i in self.items)
