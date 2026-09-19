import java.util.ArrayList;
import java.util.HashMap;

public class Main {
    enum Dir {
        North, South, East, West;

        static Dir parse(String word) {
            switch (word) {
                case "n": return Dir.North;
                case "s": return Dir.South;
                case "e": return Dir.East;
                case "w": return Dir.West;
                default: return null;
            }
        }
    }

    static class Room {
        String name;
        String item;
        HashMap<String, String> exits = new HashMap<>();
        boolean locked = false;

        Room(String name, String item) {
            this.name = name;
            this.item = item;
        }

        void link(Dir d, String to) {
            exits.put(d.name(), to);
        }
    }

    static class Game {
        private HashMap<String, Room> rooms = new HashMap<>();
        private String here = "hall";
        private ArrayList<String> bag = new ArrayList<>();
        private int moves = 0;
        boolean won = false;

        Game() {
            Room hall = new Room("hall", "");
            Room library = new Room("library", "key");
            Room vault = new Room("vault", "gold");
            Room garden = new Room("garden", "rope");
            vault.locked = true;
            hall.link(Dir.North, "library");
            hall.link(Dir.East, "vault");
            library.link(Dir.South, "hall");
            library.link(Dir.West, "garden");
            garden.link(Dir.East, "library");
            vault.link(Dir.West, "hall");
            rooms.put("hall", hall);
            rooms.put("library", library);
            rooms.put("vault", vault);
            rooms.put("garden", garden);
        }

        private Room room() {
            return rooms.get(here);
        }

        String run(String command) {
            moves++;
            String[] parts = command.split(" ");
            String verb = parts[0];
            if (verb.equals("go")) {
                Dir d = Dir.parse(parts[1]);
                if (d == null) {
                    return "which way?";
                }
                String target = room().exits.get(d.name());
                if (target == null) {
                    return "no exit " + d.name();
                }
                Room next = rooms.get(target);
                if (next.locked) {
                    if (bag.contains("key")) {
                        next.locked = false;
                        here = next.name;
                        return "unlocked " + next.name;
                    }
                    return next.name + " is locked";
                }
                here = next.name;
                return "you are in the " + here;
            }
            if (verb.equals("take")) {
                Room r = room();
                if (r.item.equals("")) {
                    return "nothing here";
                }
                bag.add(r.item);
                String got = r.item;
                r.item = "";
                if (got.equals("gold")) {
                    won = true;
                }
                return "took " + got;
            }
            if (verb.equals("look")) {
                Room r = room();
                String item = r.item.equals("") ? "nothing" : r.item;
                return r.name + " holds " + item;
            }
            return "unknown command " + verb;
        }

        String summary() {
            String items = "";
            for (int i = 0; i < bag.size(); i++) {
                if (i > 0) {
                    items += ",";
                }
                items += bag.get(i);
            }
            return "moves=" + moves + " bag=[" + items + "] won=" + won;
        }
    }

    public static void main(String[] args) {
        Game game = new Game();
        String[] script = {
            "look", "go e", "go n", "take", "go x", "go w", "take",
            "look", "go e", "go s", "go e", "take", "dance"
        };
        for (String line : script) {
            System.out.println("> " + line);
            System.out.println(game.run(line));
            if (game.won) {
                System.out.println("escaped with the gold");
                break;
            }
        }
        System.out.println(game.summary());
    }
}
