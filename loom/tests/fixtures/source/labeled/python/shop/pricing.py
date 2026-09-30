TAX = 0.2


def line(item):
    return item.price * item.qty


def apply_tax(amount):
    return round(amount * (1 + TAX), 2)


def total(items):
    def line(item):
        return apply_tax(item.price) * item.qty

    return sum(line(i) for i in items)


def summarize(items):
    return [line(i) for i in items]
