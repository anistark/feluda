:description: Integrate Feluda with Jenkins for automated license compliance.

.. _jenkins:

Jenkins
=======

.. rst-class:: lead

   Automate license compliance in Jenkins pipelines with Feluda's CI-formatted output.

----

Quick Start
-----------

Add a Feluda stage to your Jenkinsfile:

.. code-block:: groovy

   stage('Feluda Scan') {
     steps {
       sh 'feluda --ci-format jenkins --fail-on-restrictive --fail-on-incompatible'
     }
   }

Feluda emits Jenkins-friendly markers for improved log parsing and highlighting.

----

Pipeline Examples
-----------------

Declarative Pipeline
^^^^^^^^^^^^^^^^^^^^

.. code-block:: groovy

   pipeline {
     agent any

     stages {
       stage('Checkout') {
         steps {
           checkout scm
         }
       }

       stage('Feluda Scan') {
         steps {
           sh '''
             feluda --ci-format jenkins --fail-on-restrictive --fail-on-incompatible
           '''
         }
       }

       stage('Generate Compliance Artifacts') {
         steps {
           sh '''
             echo "1" | feluda generate
             echo "2" | feluda generate
             feluda sbom spdx --output sbom.spdx.json
             feluda sbom cyclonedx --output sbom.cyclonedx.json
           '''
         }
       }

       stage('Archive Artifacts') {
         steps {
           archiveArtifacts artifacts: 'NOTICE,THIRD_PARTY_LICENSES.md,sbom.*.json', fingerprint: true
         }
       }
     }
   }

Scripted Pipeline
^^^^^^^^^^^^^^^^^

.. code-block:: groovy

   node {
     stage('Checkout') {
       checkout scm
     }

     stage('Feluda Scan') {
       sh '''
         feluda --ci-format jenkins --fail-on-restrictive --fail-on-incompatible
         feluda sbom --output build/sboms
       '''
       archiveArtifacts artifacts: 'NOTICE,THIRD_PARTY_LICENSES.md,build/sboms/*', fingerprint: true
     }
   }

----

Full Compliance Pipeline
------------------------

Complete pipeline with validation and artifact archiving:

.. code-block:: groovy

   pipeline {
     agent any

     environment {
       GITHUB_TOKEN = credentials('github-token')
     }

     stages {
       stage('Checkout') {
         steps {
           checkout scm
         }
       }

       stage('License Scan') {
         steps {
           sh 'feluda --ci-format jenkins --fail-on-restrictive --fail-on-incompatible'
         }
       }

       stage('Generate Artifacts') {
         steps {
           sh '''
             echo "1" | feluda generate
             echo "2" | feluda generate
           '''
         }
       }

       stage('Generate SBOMs') {
         steps {
           sh '''
             mkdir -p build/sboms
             feluda sbom spdx --output build/sboms/sbom.spdx.json
             feluda sbom cyclonedx --output build/sboms/sbom.cyclonedx.json
           '''
         }
       }

       stage('Validate SBOMs') {
         steps {
           sh '''
             feluda sbom validate build/sboms/sbom.spdx.json --output build/sboms/spdx-validation.txt
             feluda sbom validate build/sboms/sbom.cyclonedx.json --output build/sboms/cyclonedx-validation.txt
           '''
         }
       }

       stage('Archive') {
         steps {
           archiveArtifacts artifacts: 'NOTICE,THIRD_PARTY_LICENSES.md,build/sboms/*', fingerprint: true
         }
       }
     }

     post {
       failure {
         echo 'License compliance check failed!'
       }
     }
   }

----

Scan Remote Repository
----------------------

Scan an external repository in Jenkins:

.. code-block:: groovy

   stage('Scan External Repo') {
     environment {
       SSH_PASSPHRASE = credentials('ssh-passphrase')
       HTTPS_TOKEN = credentials('github-token')
     }
     steps {
       sh '''
         feluda --repo git@github.com:org/private-repo.git \
           --ssh-key "$HOME/.ssh/ci_key" \
           --ssh-passphrase "$SSH_PASSPHRASE" \
           --ci-format jenkins
       '''
     }
   }

----

Environment Configuration
-------------------------

Configure GitHub token for rate limit management:

.. code-block:: groovy

   environment {
     GITHUB_TOKEN = credentials('github-token')
   }

Or pass inline:

.. code-block:: groovy

   sh 'feluda --github-token $GITHUB_TOKEN --ci-format jenkins'

----

Freestyle Job
-------------

For Jenkins Freestyle projects, add a build step with:

.. code-block:: bash

   feluda --ci-format jenkins --fail-on-restrictive --fail-on-incompatible
   echo "1" | feluda generate
   echo "2" | feluda generate
   feluda sbom --output sboms

Then configure "Archive the artifacts" post-build action with:

.. code-block:: text

   NOTICE,THIRD_PARTY_LICENSES.md,sboms/*
