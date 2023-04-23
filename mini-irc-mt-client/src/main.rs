use crossterm::event;
use mini_irc_mt::handle_user_input;
use mini_irc_protocol::{ChanOp, Request, Response, TypedReader, TypedWriter};
use mini_irc_ui::{App, KeyReaction};
use std::env;
use std::error::Error;
use std::net::Shutdown;
use std::ops::{DerefMut, Deref};
use std::thread::spawn;
use std::time::Instant;

enum Event {
    TerminalEvent(event::Event),
    ServerResponse(Response),
}

fn main() -> Result<(), Box<dyn Error>> {
    // Initialisation pour les logs d'erreurs.
    let start_time = Instant::now();

    let args: Vec<String> = env::args().collect();
    // Premier argument: l'addresse du serveur
    // Deuxième argument: nickname
    if args.len() != 3 {
        println!("Utilisation: ./client adresse-serveur:port nom_utilisateur");
        return Ok(());
    }

    let nickname = &args[2];
    // On se connecte au serveur
    let tcp_stream = std::net::TcpStream::connect(&args[1])?;

    // On envoie le nom d'utilisateur, pour vérifier qu'il n'est pas déjà pris.
    let mut typed_tcp_tx = TypedWriter::new(tcp_stream.try_clone()?);
    let mut typed_tcp_rx = TypedReader::new(tcp_stream.try_clone()?);

    typed_tcp_tx.send(&Request::Connect(nickname.clone()))?;

    // On vérifie la réponse
    let nickname_response = typed_tcp_rx.recv()?;

    match nickname_response {
        Some(Response::AckConnect(_)) => { /* Tout s'est bien passé */ }
        Some(Response::Error(msg)) => {
            println!("Message du serveur : {msg}");
            return Ok(());
        }
        _ => {
            println!("Réponse inattendue du serveur : {nickname_response:?}");
            return Ok(());
        }
    }
    // Et puis, on join le chan general
    typed_tcp_tx.send(&Request::JoinChan("general".into()))?;

    // Ok, tout s'est bien passé !

    // On crée deux channels pour que les threads puissent communiquer entre eux
    let (ui_output_tx, ui_output_rx) = std::sync::mpsc::channel();
    let (ui_input_tx, ui_input_rx) = std::sync::mpsc::channel();

    // On envoie la partie récepction dans son thread.
    // Cette partie lit simplement en boucle sur la socket, et envoie les données dans
    // le channel
    let tcp_reader = {
        let ui_input_tx = ui_input_tx.clone();
        spawn(move || {
            while let Ok(Some(response)) = typed_tcp_rx.recv() {
                if ui_input_tx.send(Event::ServerResponse(response)).is_err() {
                    // Il y a eu une erreur, on arrête tout
                    break;
                }
            }
        })
    };
    // L'inverse pour la partie émission : on lit sur le channel, et on envoie sur la socket
    let tcp_writer = spawn(move || {
        while let Ok(request) = ui_output_rx.recv() {
            if typed_tcp_tx.send(&request).is_err() {
                // Il y a eu une erreur, on arrête tout
                break;
            }
        }
    });

    // Etape 1: créer la structure
    let mut app = App::default();

    // Etape 2: on démarre la TUI
    app.start().unwrap();
    app.draw().unwrap();

    // Ein, un dernier thread pour les évènements du terminal
    let _terminal_event_handler = spawn(move || {
        while let Ok(e) = event::read() {
            if ui_input_tx.send(Event::TerminalEvent(e)).is_err() {
                break;
            }
        }
    });

    // Toute la partie IO est maintenant gérée. Il suffit de gérer maintenant les
    // requêtes de sources différentes (à faire)

    loop {
        // Etape 3: on dessine l'application (à faire après chaque évènement lu,
        // y compris des changements de taille de la fenêtre !)
        app.draw()?;
        let msg = ui_input_rx.recv()?;
        match msg {
            Event::TerminalEvent(e) => {
                match app.react_to_event(e) {
                    Some(KeyReaction::Quit) => {
                        break;
                    }
                    Some(KeyReaction::UserInput(input)) => {
                        // On gère l'input de l'utilisateur.
                        match handle_user_input(input, &mut app) {
                            // Requête à envoyer au serveur.
                            Ok(Some(req)) => {
                                let _ = ui_output_tx.send(req);
                            }
                            // Aucune action à réaliser.
                            Ok(None) => {}
                            // On affiche l'erreur.
                            Err(e) => {
                                let time = start_time.elapsed();
                                let notif =
                                    format!("{},{}s: {}", time.as_secs(), time.subsec_millis(), e);
                                app.set_notification(notif);
                            }
                        };
                    }
                    None => {} // Géré en interne
                }
            }
            Event::ServerResponse(response) => {
                match response {
                    Response::DirectMessage { from, content } => {
                        let user_tab = format!("@{from}");
                        let users = vec![nickname.clone(), from.clone()];
                        app.add_tab_with_users(user_tab.clone(), users);
                        app.push_message(from, content, user_tab.clone());
                    }
                    Response::AckJoin { chan, users } => {
                        let tab = format!("#{chan}");
                        app.add_tab_with_users(tab.clone(), users);
                    }
                    Response::AckLeave(chan) => {
                        app.remove_tab(format!("#{chan}"));
                    }
                    Response::Channel { op, chan } => {
                        let chan = format!("#{chan}");
                        match op {
                            ChanOp::Message { from, content } => {
                                app.push_message(from, content, chan)
                            }
                            ChanOp::UserAdd(nickname) => app.add_user(nickname, chan),
                            ChanOp::UserDel(nickname) => app.remove_user(&nickname, chan),
                        }
                    }
                    
                    _ => {
                        // on, ignore pour l'instant
                        todo!()
                    }
                }
            }
        }
    }

    // Extinction: les canaux internes doivent retourner une variante d'erreur
    drop(ui_output_tx);
    tcp_stream.shutdown(Shutdown::Both)?;
    let _ = tcp_reader.join();
    let _ = tcp_writer.join();

    // Ce n'est malheureusement pas possible pour le gestionnaire d'évènements: il
    // ne peut se fermer qu'en recevant un évènement aditionnel, et ce n'est pas très propre...

    // drop(ui_input_rx);
    // let _ = _terminal_event_handler.join();
    Ok(())
}
