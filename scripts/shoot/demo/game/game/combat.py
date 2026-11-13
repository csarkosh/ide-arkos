"""One exchange of blows."""


def damage(attack: int, armour: int, critical: bool = False) -> int:
    """What a hit takes off: the attack less the armour, at least 1, doubled on a critical."""
    if critical:
        attack *= 2
    return max(attack - armour, 1)


def turns_to_beat(health: int, attack: int, armour: int) -> int:
    """How many plain hits it takes to bring `health` to zero."""
    per_hit = damage(attack, armour)
    return -(-health // per_hit)
