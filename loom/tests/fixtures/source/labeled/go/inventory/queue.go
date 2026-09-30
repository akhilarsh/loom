package inventory

type queue []string

func (q queue) len() int {
	return len(q)
}
