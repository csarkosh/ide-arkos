from game.world import ROOMS

if __name__ == "__main__":
    for room in ROOMS:
        print(f"{room.name}: {room.enemy}")
